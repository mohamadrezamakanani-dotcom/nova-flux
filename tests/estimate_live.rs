//! Sample-based estimates and source identity through real bundled FFmpeg.
//! Run with NOVA_TEST_RUNTIME and --ignored --test-threads=1.
//! The Unix wrappers record real invocations; they never fabricate encoded media.
#![cfg(unix)]

use nova_flux::engine::{Engine, Job, MeasuredEstimate, Settings, Snapshot};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

fn runtime() -> PathBuf {
    std::env::var_os("NOVA_TEST_RUNTIME")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../media-runtime/runtime"))
}

fn settings(directory: &Path, format: &str) -> Settings {
    serde_json::from_value(serde_json::json!({
        "format": format, "quality": "medium", "output_dir": directory,
        "concurrency": 1, "performance": "cpu", "target_bytes": null
    }))
    .unwrap()
}

fn video(runtime: &Path, output: &Path, seconds: u32, dimensions: &str) {
    let result = Command::new(runtime.join("bin/ffmpeg"))
        .args(["-v", "error", "-nostdin", "-f", "lavfi", "-i"])
        .arg(format!(
            "testsrc2=size={dimensions}:rate=24:duration={seconds}"
        ))
        .args(["-f", "lavfi", "-i"])
        .arg(format!(
            "sine=frequency=617:sample_rate=48000:duration={seconds}"
        ))
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-crf",
            "14",
            "-c:a",
            "aac",
        ])
        .arg(output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "fixture: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn audio(runtime: &Path, output: &Path) {
    let result = Command::new(runtime.join("bin/ffmpeg"))
        .args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=0.2*sin(2*PI*617*t)+0.1*sin(2*PI*1931*t):s=48000:d=2",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "audio fixture: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn silent_video(runtime: &Path, output: &Path) {
    let result = Command::new(runtime.join("bin/ffmpeg"))
        .args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=24:duration=2",
            "-an",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-crf",
            "14",
        ])
        .arg(output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "silent video fixture: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\"'\"'"))
}

struct ObservedRuntime {
    directory: tempfile::TempDir,
    outputs: PathBuf,
}

impl ObservedRuntime {
    fn new(real: &Path) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let bin = directory.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let outputs = directory.path().join("ffmpeg-output-paths");
        // FFmpeg's final argument is its output. Recording it lets each test
        // inspect only the temporary artifacts owned by its own measurement.
        std::fs::write(
            bin.join("ffmpeg"),
            format!(
                "#!/bin/sh\nlast=\nfor argument in \"$@\"; do last=$argument; done\ncase \"$last\" in\n  /*) printf '%s\\n' \"$last\" >> {} ;;\nesac\nexec {} \"$@\"\n",
                quote(&outputs), quote(&real.join("bin/ffmpeg"))
            ),
        )
        .unwrap();
        std::fs::write(
            bin.join("ffprobe"),
            format!(
                "#!/bin/sh\nexec {} \"$@\"\n",
                quote(&real.join("bin/ffprobe"))
            ),
        )
        .unwrap();
        for executable in ["ffmpeg", "ffprobe"] {
            std::fs::set_permissions(bin.join(executable), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        Self { directory, outputs }
    }

    fn path(&self) -> PathBuf {
        self.directory.path().into()
    }

    fn encoded_paths(&self) -> Vec<PathBuf> {
        std::fs::read_to_string(&self.outputs)
            .unwrap_or_default()
            .lines()
            .map(PathBuf::from)
            .collect()
    }

    fn hold_completed_encode(&self, real: &Path) -> (PathBuf, PathBuf) {
        let encoded = self.directory.path().join("real-encode-complete");
        let release = self.directory.path().join("release-encode-result");
        // Preserve real FFmpeg's result while holding its caller immediately
        // before success. The bounded barrier permits deterministic source
        // replacement without corrupting the encoder's live input reads.
        std::fs::write(
            self.directory.path().join("bin/ffmpeg"),
            format!(
                "#!/bin/sh\nlast=\nfor argument in \"$@\"; do last=$argument; done\n{} \"$@\"\nresult=$?\ncase \"$last\" in\n  /*)\n    printf '%s\\n' \"$last\" >> {}\n    if [ \"$result\" = 0 ]; then\n      printf 'complete\\n' > {}\n      attempts=0\n      while [ ! -e {} ] && [ \"$attempts\" -lt 1000 ]; do\n        /bin/sleep 0.02\n        attempts=$((attempts + 1))\n      done\n    fi\n    ;;\nesac\nexit \"$result\"\n",
                quote(&real.join("bin/ffmpeg")), quote(&self.outputs), quote(&encoded), quote(&release)
            ),
        )
        .unwrap();
        (encoded, release)
    }

    fn assert_measurements_cleaned(&self) {
        let encoded = self.encoded_paths();
        assert!(!encoded.is_empty(), "no real sample encode was observed");
        for output in encoded {
            assert!(
                !output.exists(),
                "sample output leaked: {}",
                output.display()
            );
            let private = output
                .ancestors()
                .find(|path| {
                    path.file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with(".nova-estimate"))
                })
                .expect("measurement did not use a private .nova-estimate workspace");
            assert!(
                !private.exists(),
                "private sampling directory leaked: {}",
                private.display()
            );
        }
    }
}

fn import(engine: &Engine, input: &Path) -> Job {
    let snapshot = engine.import(vec![input.into()]).unwrap();
    let job = snapshot.jobs.last().unwrap();
    assert_eq!(
        job.status, "queued",
        "fixture import failed: {:?}",
        job.error
    );
    assert!(
        !job.source_revision.is_empty(),
        "import omitted source identity"
    );
    job.clone()
}

fn assert_sample(estimate: &MeasuredEstimate, encoder: &str) {
    assert!(
        estimate.estimate.bytes > 0,
        "real encode yielded no measured bytes"
    );
    assert!(
        !estimate.estimate.is_limit,
        "a sample was presented as a size limit"
    );
    assert!(estimate.estimate.lower_bytes <= estimate.estimate.bytes);
    assert!(estimate.estimate.upper_bytes >= estimate.estimate.bytes);
    assert!(
        estimate
            .estimate
            .method
            .to_ascii_lowercase()
            .contains("sample"),
        "sample provenance is missing: {}",
        estimate.estimate.method
    );
    assert_eq!(estimate.encoder, encoder);
    assert!(estimate.sampled_seconds.is_finite() && estimate.sampled_seconds > 0.0);
}

fn finish(engine: &Engine) -> Snapshot {
    let began = Instant::now();
    loop {
        let snapshot = engine.snapshot();
        if !snapshot.running {
            return snapshot;
        }
        if began.elapsed() > Duration::from_secs(60) {
            engine.shutdown();
            panic!("source/estimate acceptance conversion exceeded 60 seconds");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn decoded_size(runtime: &Path, job: &Job) -> u64 {
    assert_eq!(job.status, "done", "conversion failed: {:?}", job.error);
    let output = Path::new(job.output.as_deref().unwrap());
    let bytes = std::fs::metadata(output).unwrap().len();
    assert!(bytes > 0);
    assert_eq!(job.output_size, Some(bytes));
    let decode = Command::new(runtime.join("bin/ffmpeg"))
        .args(["-v", "error", "-xerror", "-nostdin", "-i"])
        .arg(output)
        .args(["-map", "0", "-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decode.status.success(),
        "decode: {}",
        String::from_utf8_lossy(&decode.stderr)
    );
    bytes
}

fn assert_whole_sample_close(measured: &MeasuredEstimate, actual: u64) {
    let difference = measured.estimate.bytes.abs_diff(actual);
    assert!(
        difference as f64 / actual as f64 <= 0.05,
        "whole-source sample {} bytes differs from actual {actual} bytes by more than 5%",
        measured.estimate.bytes
    );
    assert_eq!(measured.samples, 1);
    assert_eq!(measured.estimate.lower_bytes, measured.estimate.bytes);
    assert_eq!(measured.estimate.upper_bytes, measured.estimate.bytes);
}

#[test]
#[ignore = "requires real bundled FFmpeg and sample encoding"]
fn cpu_estimate_encodes_three_short_samples_without_mutating_source_or_queue() {
    let real = runtime();
    let observed = ObservedRuntime::new(&real);
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("long-source.mp4");
    video(&real, &input, 9, "640x360");
    let original = std::fs::read(&input).unwrap();
    let engine = Engine::with_performance_cache(observed.path(), None);
    let job = import(&engine, &input);
    let measured = engine
        .measure_estimate(
            &job,
            &settings(outputs.path(), "mp4"),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_sample(&measured, "libx264");
    assert_eq!(
        measured.samples, 3,
        "long media needs separated content samples"
    );
    assert!(
        measured.sampled_seconds < 5.0,
        "sampling encoded the whole 9s source"
    );
    assert_eq!(
        observed.encoded_paths().len(),
        3,
        "reported count differs from real encodes"
    );
    observed.assert_measurements_cleaned();
    assert_eq!(std::fs::read(&input).unwrap(), original);
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 0);
    let snapshot = engine.snapshot();
    assert!(!snapshot.running);
    assert_eq!(snapshot.jobs[0].status, "queued");
    assert!(snapshot.jobs[0].output.is_none());
    assert_eq!(snapshot.jobs[0].source_revision, job.source_revision);
    engine.shutdown();
    eprintln!(
        "real estimate: encoder={} samples={} sampled_seconds={:.3} projected_bytes={} observed_range={}..{}",
        measured.encoder, measured.samples, measured.sampled_seconds, measured.estimate.bytes,
        measured.estimate.lower_bytes, measured.estimate.upper_bytes
    );
}

#[test]
#[ignore = "requires real bundled FFmpeg and production encoding"]
fn whole_short_cpu_sample_matches_the_actual_completed_encode() {
    let real = runtime();
    let observed = ObservedRuntime::new(&real);
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("short-source.mp4");
    video(&real, &input, 2, "640x360");
    let original = std::fs::read(&input).unwrap();
    let engine = Engine::with_performance_cache(observed.path(), None);
    let job = import(&engine, &input);
    let configuration = settings(outputs.path(), "mp4");
    let measured = engine
        .measure_estimate(&job, &configuration, &AtomicBool::new(false))
        .unwrap();
    assert_sample(&measured, "libx264");
    assert!((measured.sampled_seconds - job.duration).abs() < 0.1);
    observed.assert_measurements_cleaned();
    engine.start(configuration).unwrap();
    let snapshot = finish(&engine);
    engine.shutdown();
    let bytes = decoded_size(&real, &snapshot.jobs[0]);
    assert_whole_sample_close(&measured, bytes);
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 1);
    assert_eq!(std::fs::read(&input).unwrap(), original);
    eprintln!(
        "whole sample acceptance: estimate={} actual={bytes}",
        measured.estimate.bytes
    );
}

#[test]
#[ignore = "requires real bundled FFmpeg with FLAC and Vorbis"]
fn variable_audio_estimates_are_measured_from_real_flac_and_vorbis_outputs() {
    let real = runtime();
    let sources = tempfile::tempdir().unwrap();
    let input = sources.path().join("audio-source.wav");
    audio(&real, &input);
    let original = std::fs::read(&input).unwrap();
    for (format, encoder) in [("flac", "flac"), ("ogg", "libvorbis")] {
        let observed = ObservedRuntime::new(&real);
        let outputs = tempfile::tempdir().unwrap();
        let engine = Engine::with_performance_cache(observed.path(), None);
        let job = import(&engine, &input);
        let configuration = settings(outputs.path(), format);
        let measured = engine
            .measure_estimate(&job, &configuration, &AtomicBool::new(false))
            .unwrap();
        assert_sample(&measured, encoder);
        assert!((measured.sampled_seconds - 2.0).abs() < 0.1);
        observed.assert_measurements_cleaned();
        engine.start(configuration).unwrap();
        let snapshot = finish(&engine);
        engine.shutdown();
        let bytes = decoded_size(&real, &snapshot.jobs[0]);
        assert_whole_sample_close(&measured, bytes);
        assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 1);
        eprintln!(
            "audio sample acceptance: format={format} measured={} actual={bytes}",
            measured.estimate.bytes
        );
    }
    assert_eq!(std::fs::read(input).unwrap(), original);
}

#[test]
#[ignore = "requires real bundled FFmpeg and a complete long-GOP remux"]
fn long_gop_copy_estimate_matches_complete_remux_without_seek_preroll_extrapolation() {
    let real = runtime();
    let observed = ObservedRuntime::new(&real);
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("long-gop-copy-source.mp4");
    let fixture = Command::new(real.join("bin/ffmpeg"))
        .args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=24:duration=20",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=617:sample_rate=48000:duration=20",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-crf",
            "14",
            "-g",
            "1000",
            "-keyint_min",
            "1000",
            "-sc_threshold",
            "0",
            "-c:a",
            "aac",
        ])
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        fixture.status.success(),
        "{}",
        String::from_utf8_lossy(&fixture.stderr)
    );
    let keys = Command::new(real.join("bin/ffprobe"))
        .args([
            "-v",
            "error",
            "-select_streams",
            "V:0",
            "-skip_frame",
            "nokey",
            "-show_frames",
            "-show_entries",
            "frame=key_frame",
            "-of",
            "json",
        ])
        .arg(&input)
        .output()
        .unwrap();
    let keys: serde_json::Value = serde_json::from_slice(&keys.stdout).unwrap();
    assert_eq!(
        keys["frames"].as_array().unwrap().len(),
        1,
        "fixture needs one long GOP"
    );
    let original = std::fs::read(&input).unwrap();
    let engine = Engine::with_performance_cache(observed.path(), None);
    let job = import(&engine, &input);
    let mut configuration = settings(outputs.path(), "mp4");
    configuration.quality = "original".into();
    let measured = engine
        .measure_estimate(&job, &configuration, &AtomicBool::new(false))
        .unwrap();
    assert_sample(&measured, "stream copy");
    observed.assert_measurements_cleaned();
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 0);
    engine.start(configuration).unwrap();
    let completed = finish(&engine);
    let actual = decoded_size(&real, &completed.jobs[0]);
    assert_eq!(measured.estimate.bytes, actual,
        "copy preview must measure the complete remux, not multiply preroll packet bytes by a short nominal duration");
    assert_eq!(measured.estimate.lower_bytes, actual);
    assert_eq!(measured.estimate.upper_bytes, actual);
    assert_eq!(measured.samples, 1);
    assert!((measured.sampled_seconds - job.duration).abs() < 0.15);
    assert_eq!(std::fs::read(input).unwrap(), original);
    engine.shutdown();
    eprintln!(
        "long-GOP copy: source={} measured={} actual={} seconds={:.3}",
        original.len(),
        measured.estimate.bytes,
        actual,
        measured.sampled_seconds
    );
}

#[test]
#[ignore = "requires real bundled FFmpeg and whole-file stream copy"]
fn whole_short_original_copy_sample_matches_the_actual_remux() {
    let real = runtime();
    let observed = ObservedRuntime::new(&real);
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("short-copy-source.mp4");
    video(&real, &input, 2, "320x180");
    let original = std::fs::read(&input).unwrap();
    let engine = Engine::with_performance_cache(observed.path(), None);
    let job = import(&engine, &input);
    let mut configuration = settings(outputs.path(), "mp4");
    configuration.quality = "original".into();
    let measured = engine
        .measure_estimate(&job, &configuration, &AtomicBool::new(false))
        .unwrap();
    assert_sample(&measured, "stream copy");
    assert!((measured.sampled_seconds - job.duration).abs() < 0.15);
    observed.assert_measurements_cleaned();
    engine.start(configuration).unwrap();
    let snapshot = finish(&engine);
    engine.shutdown();
    let bytes = decoded_size(&real, &snapshot.jobs[0]);
    assert_whole_sample_close(&measured, bytes);
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 1);
    assert_eq!(std::fs::read(input).unwrap(), original);
}

#[test]
#[ignore = "requires real bundled FFmpeg and whole-file PCM/CBR audio samples"]
fn whole_wav_and_configured_audio_samples_match_actual_completed_files() {
    let real = runtime();
    let sources = tempfile::tempdir().unwrap();
    let input = sources.path().join("whole-audio-source.wav");
    audio(&real, &input);
    let original = std::fs::read(&input).unwrap();
    for (format, encoder) in [("wav", "pcm_s16le"), ("mp3", "libmp3lame"), ("m4a", "aac")] {
        let observed = ObservedRuntime::new(&real);
        let outputs = tempfile::tempdir().unwrap();
        let engine = Engine::with_performance_cache(observed.path(), None);
        let job = import(&engine, &input);
        let configuration = settings(outputs.path(), format);
        let measured = engine
            .measure_estimate(&job, &configuration, &AtomicBool::new(false))
            .unwrap();
        assert_sample(&measured, encoder);
        assert!((measured.sampled_seconds - 2.0).abs() < 0.15);
        assert_eq!(observed.encoded_paths().len(), 1);
        observed.assert_measurements_cleaned();
        engine.start(configuration).unwrap();
        let snapshot = finish(&engine);
        engine.shutdown();
        let bytes = decoded_size(&real, &snapshot.jobs[0]);
        assert_whole_sample_close(&measured, bytes);
        assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 1);
        eprintln!(
            "whole configured audio acceptance: format={format} measured={} actual={bytes}",
            measured.estimate.bytes
        );
    }
    assert_eq!(std::fs::read(input).unwrap(), original);
}

#[test]
#[ignore = "requires real bundled FFmpeg and video-only sample encoding"]
fn silent_video_measurement_and_completed_output_preserve_the_video_only_profile() {
    let real = runtime();
    let observed = ObservedRuntime::new(&real);
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("silent-source.mp4");
    silent_video(&real, &input);
    let original = std::fs::read(&input).unwrap();
    let engine = Engine::with_performance_cache(observed.path(), None);
    let job = import(&engine, &input);
    assert!(job.video.is_some());
    assert!(job.audio.is_none());
    let configuration = settings(outputs.path(), "mp4");
    let measured = engine
        .measure_estimate(&job, &configuration, &AtomicBool::new(false))
        .unwrap();
    assert_sample(&measured, "libx264");
    observed.assert_measurements_cleaned();
    engine.start(configuration).unwrap();
    let snapshot = finish(&engine);
    engine.shutdown();
    let completed = &snapshot.jobs[0];
    let bytes = decoded_size(&real, completed);
    let ceiling = original.len() as u64 * 65 / 100;
    assert!(
        bytes <= ceiling,
        "final measured output exceeds the source-relative ceiling"
    );
    assert_eq!(snapshot.jobs[0].target_bytes, Some(ceiling));
    assert!(snapshot.jobs[0].target_is_preset);
    // The real initial candidate can differ from later measured refinements.
    assert_eq!(measured.estimate.lower_bytes, measured.estimate.bytes);
    assert_eq!(measured.estimate.upper_bytes, measured.estimate.bytes);
    let probe = Command::new(real.join("bin/ffprobe"))
        .args(["-v", "error", "-show_streams", "-of", "json"])
        .arg(completed.output.as_deref().unwrap())
        .output()
        .unwrap();
    assert!(probe.status.success());
    let media: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
    let streams = media["streams"].as_array().unwrap();
    assert_eq!(
        streams.len(),
        1,
        "silent media acquired an extra output stream"
    );
    assert_eq!(streams[0]["codec_type"], "video");
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 1);
    assert_eq!(std::fs::read(input).unwrap(), original);
}

#[test]
#[ignore = "requires real bundled FFmpeg to inspect audio-only input"]
fn audio_only_source_to_video_measurement_rejects_before_launching_an_encoder() {
    let real = runtime();
    let observed = ObservedRuntime::new(&real);
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("audio-only-source.wav");
    audio(&real, &input);
    let original = std::fs::read(&input).unwrap();
    let engine = Engine::with_performance_cache(observed.path(), None);
    let job = import(&engine, &input);
    assert!(job.video.is_none());
    let began = Instant::now();
    let error = engine
        .measure_estimate(
            &job,
            &settings(outputs.path(), "mp4"),
            &AtomicBool::new(false),
        )
        .expect_err("audio-only source incorrectly received a video estimate");
    assert!(
        error.to_ascii_lowercase().contains("video"),
        "incompatible source was not explained: {error}"
    );
    assert!(began.elapsed() < Duration::from_secs(2));
    assert!(observed.encoded_paths().is_empty());
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 0);
    assert_eq!(std::fs::read(input).unwrap(), original);
    assert_eq!(engine.snapshot().jobs[0].status, "queued");
    engine.shutdown();
}

#[test]
#[ignore = "requires real bundled FFmpeg for import"]
fn pre_cancelled_estimate_returns_immediately_without_starting_an_encoder() {
    let real = runtime();
    let observed = ObservedRuntime::new(&real);
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("cancel-source.mp4");
    video(&real, &input, 2, "320x180");
    let original = std::fs::read(&input).unwrap();
    let engine = Engine::with_performance_cache(observed.path(), None);
    let job = import(&engine, &input);
    let began = Instant::now();
    let result = engine.measure_estimate(
        &job,
        &settings(outputs.path(), "mp4"),
        &AtomicBool::new(true),
    );
    assert!(
        result.is_err(),
        "pre-cancelled measurement unexpectedly succeeded"
    );
    assert!(
        began.elapsed() < Duration::from_secs(1),
        "pre-cancel waited for a sample encode"
    );
    assert!(
        observed.encoded_paths().is_empty(),
        "pre-cancel launched real encoding"
    );
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 0);
    assert_eq!(std::fs::read(input).unwrap(), original);
    engine.shutdown();
}

#[test]
#[ignore = "requires real bundled FFmpeg and live sample cancellation"]
fn cancelling_a_live_sample_kills_its_real_encoder_and_removes_private_artifacts() {
    let real = runtime();
    let observed = ObservedRuntime::new(&real);
    let pid_file = observed.directory.path().join("sample-process");
    let converter = observed.directory.path().join("bin/ffmpeg");
    // Keep a genuine sample encode running long enough to cancel reliably.
    // exec preserves the PID, so cancellation must reap the real encoder rather
    // than a wrapper with an unowned FFmpeg descendant.
    std::fs::write(
        &converter,
        format!(
            "#!/bin/sh\nlast=\nfor argument in \"$@\"; do last=$argument; done\ncase \"$last\" in\n  /*)\n    printf '%s\\n' \"$last\" >> {}\n    printf '%s\\n' \"$$\" > {}\n    exec {} -readrate 0.01 \"$@\"\n    ;;\nesac\nexec {} \"$@\"\n",
            quote(&observed.outputs), quote(&pid_file),
            quote(&real.join("bin/ffmpeg")), quote(&real.join("bin/ffmpeg"))
        ),
    )
    .unwrap();
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("live-cancel-source.mp4");
    video(&real, &input, 2, "640x360");
    let original = std::fs::read(&input).unwrap();
    let engine = Arc::new(Engine::with_performance_cache(observed.path(), None));
    let job = import(&engine, &input);
    let cancel = Arc::new(AtomicBool::new(false));
    let measurement = {
        let engine = engine.clone();
        let cancel = cancel.clone();
        let configuration = settings(outputs.path(), "mp4");
        thread::spawn(move || engine.measure_estimate(&job, &configuration, &cancel))
    };
    let began = Instant::now();
    loop {
        // Verify exec completed: the wrapper's own command line never contains
        // the injected readrate option. The encoder may buffer video frames
        // before producing bytes, so output length is not a startup signal.
        let executing_real_encoder = std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|pid| {
                Command::new("/bin/ps")
                    .args(["-p", pid.trim(), "-o", "command="])
                    .output()
                    .ok()
            })
            .is_some_and(|process| {
                let command = String::from_utf8_lossy(&process.stdout);
                process.status.success()
                    && command.contains(&real.join("bin/ffmpeg").to_string_lossy().to_string())
                    && command.contains("-readrate 0.01")
            });
        if executing_real_encoder {
            break;
        }
        if measurement.is_finished() || began.elapsed() > Duration::from_secs(12) {
            cancel.store(true, Ordering::Release);
            let _ = measurement.join();
            engine.shutdown();
            panic!("no real FFmpeg sample process started before cancellation");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let pid = std::fs::read_to_string(&pid_file).unwrap();
    let cancelled_at = Instant::now();
    cancel.store(true, Ordering::Release);
    let error = measurement
        .join()
        .expect("sample worker panicked")
        .expect_err("cancelled sample unexpectedly produced an estimate");
    assert!(
        cancelled_at.elapsed() < Duration::from_secs(2),
        "sample cancellation waited for its encoding deadline"
    );
    assert!(
        error.to_ascii_lowercase().contains("cancel"),
        "cancellation was not explained: {error}"
    );
    let alive = Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .output()
        .unwrap()
        .status
        .success();
    assert!(!alive, "cancelled real FFmpeg process survived cleanup");
    assert_eq!(observed.encoded_paths().len(), 1);
    observed.assert_measurements_cleaned();
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 0);
    assert_eq!(std::fs::read(&input).unwrap(), original);
    assert_eq!(engine.snapshot().jobs[0].status, "queued");
    engine.shutdown();
    eprintln!(
        "live sample cancellation acceptance: stopped_ms={}",
        cancelled_at.elapsed().as_millis()
    );
}

#[test]
#[ignore = "requires real bundled FFmpeg and source replacement"]
fn estimate_rejects_a_source_replaced_since_import_without_encoding_it() {
    let real = runtime();
    let observed = ObservedRuntime::new(&real);
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("replace-source.mp4");
    video(&real, &input, 2, "320x180");
    let engine = Engine::with_performance_cache(observed.path(), None);
    let job = import(&engine, &input);
    let replacement = sources.path().join("replacement.mp4");
    video(&real, &replacement, 4, "640x360");
    std::fs::rename(&replacement, &input).unwrap();
    let replacement_bytes = std::fs::read(&input).unwrap();
    assert_ne!(job.size, replacement_bytes.len() as u64);
    let error = engine
        .measure_estimate(
            &job,
            &settings(outputs.path(), "mp4"),
            &AtomicBool::new(false),
        )
        .expect_err("stale imported source unexpectedly received an estimate");
    assert!(
        error.to_ascii_lowercase().contains("chang"),
        "source identity failure was not explained: {error}"
    );
    assert!(
        observed.encoded_paths().is_empty(),
        "stale source was sampled"
    );
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 0);
    assert_eq!(std::fs::read(input).unwrap(), replacement_bytes);
    engine.shutdown();
}

#[test]
#[ignore = "requires real bundled FFmpeg for import"]
fn target_size_request_is_rejected_as_a_measurement_without_sample_work() {
    let real = runtime();
    let observed = ObservedRuntime::new(&real);
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("target-source.mp4");
    video(&real, &input, 2, "320x180");
    let engine = Engine::with_performance_cache(observed.path(), None);
    let job = import(&engine, &input);
    let mut configuration = settings(outputs.path(), "mp4");
    configuration.target_bytes = Some(100_000);
    let error = engine
        .measure_estimate(&job, &configuration, &AtomicBool::new(false))
        .expect_err("target ceiling was incorrectly returned as a sampled prediction");
    assert!(
        ["target", "limit"]
            .iter()
            .any(|word| error.to_ascii_lowercase().contains(word)),
        "target/sample distinction was not explained: {error}"
    );
    assert!(observed.encoded_paths().is_empty());
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 0);
    engine.shutdown();
}

#[test]
#[ignore = "requires real bundled FFmpeg and source re-analysis"]
fn source_change_during_sampling_refreshes_metadata_and_discards_the_stale_estimate() {
    let real = runtime();
    let observed = ObservedRuntime::new(&real);
    let (encoded, release) = observed.hold_completed_encode(&real);
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("sampling-change-source.mp4");
    let replacement = sources.path().join("replacement.mp4");
    video(&real, &input, 2, "320x180");
    video(&real, &replacement, 3, "640x360");
    let replacement_bytes = std::fs::read(&replacement).unwrap();
    let engine = Arc::new(Engine::with_performance_cache(observed.path(), None));
    let imported = import(&engine, &input);
    let measurement = {
        let engine = engine.clone();
        let job = imported.clone();
        let configuration = settings(outputs.path(), "mp4");
        thread::spawn(move || {
            engine.measure_estimate(&job, &configuration, &AtomicBool::new(false))
        })
    };
    let began = Instant::now();
    while !encoded.exists() {
        if measurement.is_finished() || began.elapsed() > Duration::from_secs(12) {
            std::fs::write(&release, b"release").unwrap();
            let _ = measurement.join();
            engine.shutdown();
            panic!("real sample never reached the source-change barrier");
        }
        thread::sleep(Duration::from_millis(10));
    }
    std::fs::rename(&replacement, &input).unwrap();
    std::fs::write(&release, b"release").unwrap();
    let error = measurement
        .join()
        .expect("sample worker panicked")
        .expect_err("source replacement yielded a stale successful estimate");
    assert!(
        error.to_ascii_lowercase().contains("chang"),
        "source revision failure was not explained: {error}"
    );
    let snapshot = engine.snapshot();
    let refreshed = &snapshot.jobs[0];
    assert_eq!(refreshed.size, replacement_bytes.len() as u64);
    assert_ne!(refreshed.source_revision, imported.source_revision);
    assert_eq!((refreshed.width, refreshed.height), (Some(640), Some(360)));
    assert!((refreshed.duration - 3.0).abs() < 0.15);
    assert_eq!(refreshed.status, "queued");
    assert!(refreshed.output.is_none());
    assert!(refreshed.output_size.is_none());
    assert!(!snapshot.running);
    observed.assert_measurements_cleaned();
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 0);
    assert_eq!(std::fs::read(input).unwrap(), replacement_bytes);
    engine.shutdown();
}

#[test]
#[ignore = "requires real bundled FFmpeg and source re-analysis"]
fn retry_refreshes_replaced_source_metadata_before_it_is_queued_again() {
    let real = runtime();
    let sources = tempfile::tempdir().unwrap();
    let input = sources.path().join("retry-source.mp4");
    video(&real, &input, 2, "320x180");
    let engine = Engine::with_performance_cache(real.clone(), None);
    let imported = import(&engine, &input);
    engine.cancel(Some(imported.id.clone())).unwrap();
    assert_eq!(engine.snapshot().jobs[0].status, "cancelled");
    let replacement = sources.path().join("replacement.mp4");
    video(&real, &replacement, 3, "640x360");
    std::fs::rename(&replacement, &input).unwrap();
    let replacement_bytes = std::fs::read(&input).unwrap();
    engine.retry(imported.id.clone()).unwrap();
    let snapshot = engine.snapshot();
    let retried = &snapshot.jobs[0];
    assert_eq!(retried.status, "queued");
    assert_eq!(retried.size, replacement_bytes.len() as u64);
    assert_ne!(retried.source_revision, imported.source_revision);
    assert_eq!((retried.width, retried.height), (Some(640), Some(360)));
    assert!((retried.duration - 3.0).abs() < 0.15);
    assert!(retried.output.is_none());
    assert!(retried.output_size.is_none());
    assert_eq!(std::fs::read(input).unwrap(), replacement_bytes);
    engine.shutdown();
}

#[test]
#[ignore = "requires real bundled FFmpeg and source re-analysis"]
fn conversion_refreshes_replaced_source_size_revision_and_output_dimensions() {
    let real = runtime();
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("refresh-source.mp4");
    video(&real, &input, 2, "320x180");
    let engine = Engine::with_performance_cache(real.clone(), None);
    let imported = import(&engine, &input);
    let replacement = sources.path().join("replacement.mp4");
    video(&real, &replacement, 3, "640x360");
    std::fs::rename(&replacement, &input).unwrap();
    let source_bytes = std::fs::read(&input).unwrap();
    engine.start(settings(outputs.path(), "mp4")).unwrap();
    let snapshot = finish(&engine);
    engine.shutdown();
    let completed = &snapshot.jobs[0];
    decoded_size(&real, completed);
    assert_eq!(
        completed.size,
        source_bytes.len() as u64,
        "conversion kept stale input size"
    );
    assert_ne!(completed.source_revision, imported.source_revision);
    assert_eq!((completed.width, completed.height), (Some(640), Some(360)));
    let metadata = serde_json::to_value(completed).unwrap();
    assert_eq!(metadata["output_width"], 640);
    assert_eq!(metadata["output_height"], 360);
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 1);
    assert_eq!(std::fs::read(input).unwrap(), source_bytes);
}

#[test]
#[ignore = "requires real bundled FFmpeg and a synchronized before-publish source change"]
fn source_change_after_encoding_is_rejected_before_publication() {
    let real = runtime();
    let observed = ObservedRuntime::new(&real);
    let (encoded, release) = observed.hold_completed_encode(&real);
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("publish-source.mp4");
    let replacement = sources.path().join("replacement.mp4");
    video(&real, &input, 2, "320x180");
    video(&real, &replacement, 3, "640x360");
    let replacement_bytes = std::fs::read(&replacement).unwrap();
    let engine = Engine::with_performance_cache(observed.path(), None);
    import(&engine, &input);
    engine.start(settings(outputs.path(), "mp4")).unwrap();
    let began = Instant::now();
    while !encoded.exists() {
        let snapshot = engine.snapshot();
        if !snapshot.running || began.elapsed() > Duration::from_secs(15) {
            std::fs::write(&release, b"release").unwrap();
            engine.shutdown();
            panic!(
                "real encoder never reached the publication barrier: {:?}",
                snapshot.jobs
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(engine.snapshot().running);
    std::fs::rename(&replacement, &input).unwrap();
    std::fs::write(&release, b"release").unwrap();
    let snapshot = finish(&engine);
    engine.shutdown();
    let job = &snapshot.jobs[0];
    assert_eq!(
        job.status, "error",
        "changed input was published: {:?}",
        job.output
    );
    assert!(job.output.is_none());
    assert!(job.output_size.is_none());
    assert!(
        job.error
            .as_ref()
            .is_some_and(|error| error.to_ascii_lowercase().contains("chang")),
        "source change was not explained: {:?}",
        job.error
    );
    assert_eq!(
        std::fs::read_dir(outputs.path()).unwrap().count(),
        0,
        "unverified output leaked"
    );
    assert_eq!(std::fs::read(input).unwrap(), replacement_bytes);
}

#[test]
#[ignore = "requires real bundled FFmpeg and background source re-analysis"]
fn background_refresh_invalidates_cached_source_metadata_without_new_sample_requests() {
    let real = runtime();
    let sources = tempfile::tempdir().unwrap();
    let input = sources.path().join("cached-source.mp4");
    let replacement = sources.path().join("replacement.mp4");
    video(&real, &input, 2, "320x180");
    let engine = Engine::with_performance_cache(real.clone(), None);
    let imported = import(&engine, &input);
    video(&real, &replacement, 3, "640x360");
    let bytes = std::fs::metadata(&replacement).unwrap().len();
    std::fs::rename(replacement, &input).unwrap();
    let refreshed = engine.refresh_sources().unwrap();
    let job = &refreshed.jobs[0];
    assert_eq!(job.status, "queued");
    assert_eq!(job.size, bytes);
    assert_ne!(job.source_revision, imported.source_revision);
    assert_eq!((job.width, job.height), (Some(640), Some(360)));
    let revision = job.source_revision.clone();
    assert_eq!(
        engine.refresh_sources().unwrap().jobs[0].source_revision,
        revision
    );
    std::fs::write(&input, b"changed into an invalid media file").unwrap();
    let invalid = engine.refresh_sources().unwrap();
    assert_eq!(invalid.jobs[0].status, "error");
    assert_eq!(invalid.jobs[0].size, 34);
    assert!(invalid.jobs[0]
        .error
        .as_ref()
        .is_some_and(|error| !error.is_empty()));
    engine.shutdown();
}

#[test]
#[ignore = "requires real bundled FFprobe and cancellation of background re-analysis"]
fn cancelling_background_refresh_reaps_its_inspector_without_marking_the_source_failed() {
    let real = runtime();
    let observed = ObservedRuntime::new(&real);
    let sources = tempfile::tempdir().unwrap();
    let input = sources.path().join("refresh-cancel.mp4");
    let replacement = sources.path().join("replacement.mp4");
    video(&real, &input, 2, "320x180");
    let engine = Arc::new(Engine::with_performance_cache(observed.path(), None));
    import(&engine, &input);
    video(&real, &replacement, 3, "640x360");
    std::fs::rename(replacement, &input).unwrap();
    let bytes = std::fs::read(&input).unwrap();
    let finished = observed.path().join("real-inspector-finished");
    let pid = observed.path().join("inspector-pid");
    let wrapper = observed.path().join("bin/ffprobe");
    // Retain the real inspector's output/status, then hold its command boundary
    // so cancellation is deterministic even on fast local storage.
    std::fs::write(&wrapper, format!("#!/bin/sh\nprintf '%s' \"$$\" > {}\n{} \"$@\"\nresult=$?\n: > {}\nwhile :; do :; done\nexit \"$result\"\n", quote(&pid), quote(&real.join("bin/ffprobe")), quote(&finished))).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let refresh = {
        let engine = engine.clone();
        let cancel = cancel.clone();
        thread::spawn(move || engine.refresh_sources_cancellable(&cancel))
    };
    let started = Instant::now();
    while !finished.exists() {
        if refresh.is_finished() || started.elapsed() > Duration::from_secs(5) {
            cancel.store(true, std::sync::atomic::Ordering::Release);
            let result = refresh.join();
            panic!("real inspector did not reach cancellation boundary: {result:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let cancelled_at = Instant::now();
    cancel.store(true, std::sync::atomic::Ordering::Release);
    let error = refresh.join().unwrap().unwrap_err();
    assert!(error.to_lowercase().contains("cancel"), "{error}");
    assert!(cancelled_at.elapsed() < Duration::from_secs(2));
    let process = std::fs::read_to_string(pid).unwrap();
    assert!(
        !Command::new("/bin/kill")
            .args(["-0", process.trim()])
            .output()
            .unwrap()
            .status
            .success(),
        "cancelled inspector was not reaped"
    );
    assert_eq!(engine.snapshot().jobs[0].status, "queued");
    assert_eq!(std::fs::read(input).unwrap(), bytes);
    engine.shutdown();
}

#[test]
#[ignore = "requires real bundled FFmpeg and complete-copy verification"]
fn complete_copy_measurements_reject_real_truncated_video_and_audio_remuxes() {
    let real = runtime();
    for format in ["mp4", "wav"] {
        let observed = ObservedRuntime::new(&real);
        let sources = tempfile::tempdir().unwrap();
        let outputs = tempfile::tempdir().unwrap();
        let input = sources
            .path()
            .join(format!("complete-copy-source.{format}"));
        if format == "mp4" {
            video(&real, &input, 8, "320x180");
        } else {
            let fixture = Command::new(real.join("bin/ffmpeg"))
                .args([
                    "-v",
                    "error",
                    "-nostdin",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=617:sample_rate=48000:duration=8",
                    "-c:a",
                    "pcm_s16le",
                ])
                .arg(&input)
                .output()
                .unwrap();
            assert!(fixture.status.success());
        }
        let original = std::fs::read(&input).unwrap();
        let encoder = observed.path().join("bin/ffmpeg");
        let script = std::fs::read_to_string(&encoder).unwrap();
        let real_command = format!("exec {} \"$@\"", quote(&real.join("bin/ffmpeg")));
        let truncated = script.replace(
            &real_command,
            &format!("exec {} -t 6.5 \"$@\"", quote(&real.join("bin/ffmpeg"))),
        );
        assert_ne!(
            script, truncated,
            "real truncating wrapper was not installed"
        );
        std::fs::write(&encoder, truncated).unwrap();
        let engine = Engine::with_performance_cache(observed.path(), None);
        let imported = import(&engine, &input);
        let mut configuration = settings(outputs.path(), format);
        configuration.quality = "original".into();
        let error = engine
            .measure_estimate(&imported, &configuration, &AtomicBool::new(false))
            .expect_err("an incomplete remux was incorrectly accepted as complete-copy evidence");
        let expected = if format == "mp4" {
            "frame count"
        } else {
            "substantially shorter"
        };
        assert!(
            error.contains(expected),
            "{format}: completeness failure was not explained: {error}"
        );
        observed.assert_measurements_cleaned();
        assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 0);
        assert_eq!(engine.snapshot().jobs[0].status, "queued");
        assert_eq!(std::fs::read(input).unwrap(), original);
        engine.shutdown();
    }
}
