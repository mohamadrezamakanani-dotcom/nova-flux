//! Target-size acceptance through the public desktop engine API. Run with NOVA_TEST_RUNTIME.
use nova_flux::engine::{Engine, Settings, Snapshot};
use std::{
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

fn runtime() -> PathBuf {
    std::env::var_os("NOVA_TEST_RUNTIME")
        .expect("NOVA_TEST_RUNTIME")
        .into()
}
fn settings(dir: &Path, format: &str, target: u64, concurrency: usize) -> Settings {
    // Deserialize the real wire contract; an ignored/missing target must fail behavioral assertions.
    serde_json::from_value(serde_json::json!({"format":format,"quality":"medium",
        "output_dir":dir,"concurrency":concurrency,"target_bytes":target}))
    .unwrap()
}
fn video(runtime: &Path, output: &Path, seconds: u32, large: bool) {
    let dimensions = if large {
        "640x360:rate=24"
    } else {
        "320x180:rate=12"
    };
    let result = Command::new(runtime.join("bin/ffmpeg"))
        .args(["-v", "error", "-nostdin", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=size={dimensions}:duration={seconds}"))
        .args(["-f", "lavfi", "-i"])
        .arg(format!(
            "sine=frequency=721:sample_rate=48000:duration={seconds}"
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
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
fn finish(engine: &Engine) -> Snapshot {
    let began = Instant::now();
    loop {
        let snapshot = engine.snapshot();
        if !snapshot.running {
            return snapshot;
        }
        if began.elapsed() > Duration::from_secs(90) {
            engine.shutdown();
            panic!("target-size conversion exceeded 90 seconds");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "requires real bundled FFmpeg runtime"]
fn an_explicit_audio_limit_can_exceed_source_size_without_becoming_a_reduction_goal() {
    let runtime = runtime();
    let source_dir = tempfile::tempdir().unwrap();
    let output_dir = tempfile::tempdir().unwrap();
    let input = source_dir.path().join("small-aac.m4a");
    let fixture = Command::new(runtime.join("bin/ffmpeg"))
        .args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=617:sample_rate=48000:duration=4",
            "-c:a",
            "aac",
            "-b:a",
            "24k",
        ])
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        fixture.status.success(),
        "{}",
        String::from_utf8_lossy(&fixture.stderr)
    );
    let source = std::fs::read(&input).unwrap();
    let limit = 200_000;
    assert!((source.len() as u64) < limit / 4);
    let engine = Engine::new(runtime.clone());
    engine.import(vec![input.clone()]).unwrap();
    engine
        .start(settings(output_dir.path(), "mp3", limit, 1))
        .unwrap();
    let snapshot = finish(&engine);
    engine.shutdown();
    let job = &snapshot.jobs[0];
    assert_eq!(job.status, "done", "{:?}", job.error);
    assert_eq!(job.target_bytes, Some(limit));
    assert!(!job.target_is_preset);
    let actual = verify_output(
        &runtime,
        Path::new(job.output.as_ref().unwrap()),
        limit,
        4.0,
        false,
    );
    assert!(
        actual > source.len() as u64,
        "explicit limit was silently clamped to source size"
    );
    assert_eq!(job.output_size, Some(actual));
    assert_eq!(std::fs::read(input).unwrap(), source);
    assert_eq!(std::fs::read_dir(output_dir.path()).unwrap().count(), 1);
}

fn verify_output(
    runtime: &Path,
    output: &Path,
    target: u64,
    seconds: f64,
    video_expected: bool,
) -> u64 {
    let bytes = std::fs::metadata(output).unwrap().len();
    assert!(
        bytes > 0 && bytes <= target,
        "{}: {bytes} bytes exceeds target {target}",
        output.display()
    );
    let probe = Command::new(runtime.join("bin/ffprobe"))
        .args([
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(output)
        .output()
        .unwrap();
    assert!(probe.status.success());
    let data: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
    let streams = data["streams"].as_array().unwrap();
    assert!(streams.iter().any(|s| s["codec_type"] == "audio"));
    assert_eq!(
        streams.iter().any(|s| s["codec_type"] == "video"),
        video_expected
    );
    let duration: f64 = data["format"]["duration"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        (duration - seconds).abs() <= 0.35,
        "output duration {duration} does not preserve input {seconds}"
    );
    for stream in streams {
        if let Some(duration) = stream["duration"]
            .as_str()
            .and_then(|v| v.parse::<f64>().ok())
        {
            assert!(
                (duration - seconds).abs() <= 0.35,
                "selected stream duration {duration} differs from {seconds}"
            );
        }
    }
    let decode = Command::new(runtime.join("bin/ffmpeg"))
        .args(["-v", "error", "-xerror", "-nostdin", "-i"])
        .arg(output)
        .args(["-map", "0", "-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        decode.status.success(),
        "{}",
        String::from_utf8_lossy(&decode.stderr)
    );
    eprintln!("target result: format={} target_bytes={target} output_bytes={bytes} duration={duration:.3}s", output.extension().unwrap_or_default().to_string_lossy());
    bytes
}
fn convert(
    runtime: &Path,
    input: &Path,
    output_dir: &Path,
    format: &str,
    target: u64,
    seconds: f64,
) -> u64 {
    let source = std::fs::read(input).unwrap();
    let engine = Engine::new(runtime.into());
    engine.import(vec![input.into()]).unwrap();
    engine
        .start(settings(output_dir, format, target, 1))
        .unwrap();
    let snapshot = finish(&engine);
    engine.shutdown();
    let job = &snapshot.jobs[0];
    assert_eq!(job.status, "done", "{format}/{target}: {:?}", job.error);
    let bytes = verify_output(
        runtime,
        Path::new(job.output.as_ref().unwrap()),
        target,
        seconds,
        ["mp4", "mkv", "mov", "avi", "webm"].contains(&format),
    );
    let serialized = serde_json::to_value(job).unwrap();
    assert_eq!(serialized["target_bytes"].as_u64(), Some(target));
    assert_eq!(serialized["output_size"].as_u64(), Some(bytes));
    assert_eq!(
        std::fs::read(input).unwrap(),
        source,
        "source bytes were changed"
    );
    bytes
}

#[test]
#[ignore = "requires real bundled FFmpeg"]
fn one_megabyte_video_uses_budget_without_truncation() {
    let runtime = runtime();
    let source_dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let input = source_dir.path().join("complex.mp4");
    video(&runtime, &input, 10, true);
    let bytes = convert(&runtime, &input, out.path(), "mp4", 1_000_000, 10.0);
    assert!(
        bytes >= 850_000,
        "complex 1 MB request uses only {bytes} bytes; budget should fund quality"
    );
    assert_eq!(
        std::fs::read_dir(out.path()).unwrap().count(),
        1,
        "pass files leaked"
    );
}

#[test]
#[ignore = "requires real bundled FFmpeg"]
fn tight_budgets_cover_every_lossy_target() {
    let runtime = runtime();
    let source_dir = tempfile::tempdir().unwrap();
    let input = source_dir.path().join("moving.mp4");
    video(&runtime, &input, 8, false);
    for format in [
        "mp4", "mkv", "mov", "avi", "webm", "mp3", "aac", "m4a", "ogg",
    ] {
        let out = tempfile::tempdir().unwrap();
        let target = if ["mp3", "aac", "m4a", "ogg"].contains(&format) {
            50_000
        } else {
            100_000
        };
        if format == "avi" {
            let engine = Engine::with_performance_cache(runtime.clone(), None);
            engine.import(vec![input.clone()]).unwrap();
            engine
                .start(settings(out.path(), format, target, 1))
                .unwrap();
            let snapshot = finish(&engine);
            let job = &snapshot.jobs[0];
            assert_eq!(
                job.status, "error",
                "the fixed-raster AVI budget should be refused"
            );
            assert!(job
                .error
                .as_ref()
                .is_some_and(|error| error.contains("unchanged dimensions")
                    && error.contains("Increase the limit")));
            assert!(job.output.is_none());
            assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
            engine.shutdown();
            convert(&runtime, &input, out.path(), format, 500_000, 8.0);
        } else {
            convert(&runtime, &input, out.path(), format, target, 8.0);
        }
        assert_eq!(
            std::fs::read_dir(out.path()).unwrap().count(),
            1,
            "{format}: pass files leaked"
        );
    }
}

#[test]
#[ignore = "requires real bundled FFmpeg"]
fn impossible_budget_is_a_per_file_error_without_output() {
    let runtime = runtime();
    let source_dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let input = source_dir.path().join("too-small.mp4");
    video(&runtime, &input, 6, false);
    let before = std::fs::read(&input).unwrap();
    let engine = Engine::new(runtime);
    engine.import(vec![input.clone()]).unwrap();
    engine.start(settings(out.path(), "mp4", 1_000, 1)).unwrap();
    let snapshot = finish(&engine);
    engine.shutdown();
    assert_eq!(snapshot.jobs[0].status, "error");
    assert!(snapshot.jobs[0]
        .error
        .as_ref()
        .is_some_and(|e| !e.is_empty()));
    assert!(snapshot.jobs[0].output.is_none());
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
    assert_eq!(std::fs::read(input).unwrap(), before);
}

#[test]
#[ignore = "requires real bundled FFmpeg"]
fn target_size_rejects_lossless_and_stream_copy() {
    let runtime = runtime();
    let source_dir = tempfile::tempdir().unwrap();
    let input = source_dir.path().join("source.mp4");
    video(&runtime, &input, 2, false);
    for (format, quality) in [("wav", "medium"), ("flac", "medium"), ("mp4", "original")] {
        let out = tempfile::tempdir().unwrap();
        let engine = Engine::new(runtime.clone());
        engine.import(vec![input.clone()]).unwrap();
        let mut config = settings(out.path(), format, 100_000, 1);
        config.quality = quality.into();
        if engine.start(config).is_ok() {
            let snapshot = finish(&engine);
            assert_eq!(
                snapshot.jobs[0].status, "error",
                "must reject {format}/{quality}"
            );
            assert!(snapshot.jobs[0].output.is_none());
        }
        engine.shutdown();
        assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
    }
}

#[test]
#[ignore = "requires real bundled FFmpeg"]
fn target_size_cancel_removes_outputs_and_pass_logs() {
    let runtime = runtime();
    let source_dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let input = source_dir.path().join("cancel.mp4");
    video(&runtime, &input, 15, true);
    let before = std::fs::read(&input).unwrap();
    let engine = Engine::new(runtime);
    engine.import(vec![input.clone()]).unwrap();
    engine
        .start(settings(out.path(), "webm", 1_000_000, 1))
        .unwrap();
    // Give the planner/first pass time to create its private artifacts.
    thread::sleep(Duration::from_millis(200));
    engine.cancel(None).unwrap();
    let snapshot = finish(&engine);
    engine.shutdown();
    assert_eq!(snapshot.jobs[0].status, "cancelled");
    assert!(snapshot.jobs[0].output.is_none());
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
    assert_eq!(std::fs::read(input).unwrap(), before);
}

#[test]
#[ignore = "requires real bundled FFmpeg"]
fn two_target_jobs_isolate_pass_logs_and_keep_both_outputs() {
    let runtime = runtime();
    let source_dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let input = source_dir.path().join("one.mp4");
    let second = source_dir.path().join("two.mp4");
    video(&runtime, &input, 8, false);
    std::fs::copy(&input, &second).unwrap();
    let engine = Engine::new(runtime.clone());
    engine.import(vec![input, second]).unwrap();
    engine
        .start(settings(out.path(), "mp4", 100_000, 2))
        .unwrap();
    let snapshot = finish(&engine);
    engine.shutdown();
    assert_eq!(snapshot.jobs.len(), 2);
    for job in snapshot.jobs {
        assert_eq!(job.status, "done", "{:?}", job.error);
        verify_output(
            &runtime,
            Path::new(job.output.as_ref().unwrap()),
            100_000,
            8.0,
            true,
        );
    }
    assert_eq!(
        std::fs::read_dir(out.path()).unwrap().count(),
        2,
        "pass logs leaked or outputs collided"
    );
}

#[test]
#[ignore = "requires real bundled FFmpeg"]
fn target_size_preserves_rotated_phone_video_shape() {
    let runtime = runtime();
    let source_dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let landscape = source_dir.path().join("encoded-landscape.mp4");
    let input = source_dir.path().join("phone-portrait.mp4");
    video(&runtime, &landscape, 6, false);
    let rotate = Command::new(runtime.join("bin/ffmpeg"))
        .args([
            "-v",
            "error",
            "-nostdin",
            "-display_rotation:v:0",
            "90",
            "-i",
        ])
        .arg(&landscape)
        .args(["-c", "copy"])
        .arg(&input)
        .output()
        .unwrap();
    assert!(rotate.status.success());
    let input_probe = Command::new(runtime.join("bin/ffprobe"))
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_streams",
            "-of",
            "json",
        ])
        .arg(&input)
        .output()
        .unwrap();
    let source_metadata: serde_json::Value = serde_json::from_slice(&input_probe.stdout).unwrap();
    assert!(
        source_metadata["streams"][0]["side_data_list"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["rotation"]
                .as_f64()
                .is_some_and(|angle| angle.abs() == 90.0)),
        "fixture must carry a real 90-degree display rotation"
    );
    convert(&runtime, &input, out.path(), "mp4", 100_000, 6.0);
    let output = std::fs::read_dir(out.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let probe = Command::new(runtime.join("bin/ffprobe"))
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_streams",
            "-of",
            "json",
        ])
        .arg(output)
        .output()
        .unwrap();
    assert!(probe.status.success());
    let data: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
    let stream = &data["streams"][0];
    let width = stream["width"].as_f64().unwrap();
    let height = stream["height"].as_f64().unwrap();
    let sar = stream["sample_aspect_ratio"]
        .as_str()
        .and_then(|value| {
            let (a, b) = value.split_once(':')?;
            Some(a.parse::<f64>().ok()? / b.parse::<f64>().ok()?)
        })
        .unwrap_or(1.0);
    let rotation = stream["side_data_list"]
        .as_array()
        .and_then(|items| items.iter().find_map(|item| item["rotation"].as_f64()))
        .unwrap_or(0.0);
    let mut aspect = width * sar / height;
    if rotation.abs().rem_euclid(180.0) > 45.0 {
        aspect = 1.0 / aspect;
    }
    // Both square-pixel portrait and anamorphic encoding are valid when displayed
    // with their declared pixel aspect and rotation. Check the actual display shape.
    assert!(
        (aspect - 180.0 / 320.0).abs() < 0.025,
        "phone portrait display aspect changed to {aspect} ({width}x{height}, SAR {sar})"
    );
}

#[test]
#[ignore = "requires real bundled FFmpeg"]
fn target_audio_extraction_reads_matroska_stream_duration_tags() {
    let runtime = runtime();
    let source_dir = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let mp4 = source_dir.path().join("source.mp4");
    let mkv = source_dir.path().join("source.mkv");
    video(&runtime, &mp4, 8, false);
    let remux = Command::new(runtime.join("bin/ffmpeg"))
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(&mp4)
        .args(["-c", "copy"])
        .arg(&mkv)
        .output()
        .unwrap();
    assert!(remux.status.success());
    convert(&runtime, &mkv, out.path(), "mp3", 50_000, 8.0);
}
