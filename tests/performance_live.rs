//! Real-machine acceptance for adaptive workers and hardware/CPU execution.
//! Run with NOVA_TEST_RUNTIME; no fake encoder or fabricated queue state.
use nova_flux::engine::{Engine, Settings, Snapshot};
use std::{
    collections::BTreeSet,
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
fn configuration(dir: &Path, format: &str, performance: &str, target: Option<u64>) -> Settings {
    serde_json::from_value(serde_json::json!({
        "format": format, "quality": "medium", "output_dir": dir,
        "concurrency": 0, "performance": performance, "target_bytes": target
    }))
    .unwrap()
}
fn fixture(runtime: &Path, output: &Path, seconds: u32, size: &str) {
    let result = Command::new(runtime.join("bin/ffmpeg"))
        .args(["-v", "error", "-nostdin", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=size={size}:rate=24:duration={seconds}"))
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
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
fn wait(engine: &Engine) -> (Snapshot, BTreeSet<String>, usize) {
    let deadline = Instant::now();
    let mut encoders = BTreeSet::new();
    let mut peak = 0;
    loop {
        let snapshot = engine.snapshot();
        peak = peak.max(
            snapshot
                .jobs
                .iter()
                .filter(|j| j.status == "running")
                .count(),
        );
        for job in &snapshot.jobs {
            if let Some(encoder) = serde_json::to_value(job).unwrap()["encoder"].as_str() {
                encoders.insert(encoder.to_owned());
            }
        }
        if !snapshot.running {
            return (snapshot, encoders, peak);
        }
        if deadline.elapsed() > Duration::from_secs(120) {
            engine.shutdown();
            panic!("performance-mode conversion exceeded 120 seconds");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
fn verify(runtime: &Path, output: &Path, seconds: f64, target: Option<u64>) -> u64 {
    let bytes = std::fs::metadata(output).unwrap().len();
    assert!(bytes > 0);
    if let Some(target) = target {
        assert!(
            bytes <= target,
            "{bytes} output bytes exceed strict {target} limit"
        );
    }
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
    let metadata: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
    let streams = metadata["streams"].as_array().unwrap();
    assert!(streams.iter().any(|s| s["codec_type"] == "video"));
    assert!(streams.iter().any(|s| s["codec_type"] == "audio"));
    let duration = metadata["format"]["duration"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!(
        (duration - seconds).abs() < 0.35,
        "duration {duration} lost timeline from {seconds}s source"
    );
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
    bytes
}
fn actual_hardware_available(runtime: &Path) -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    // Encoder listings alone do not prove VideoToolbox can open a real hardware session.
    let probe = Command::new(runtime.join("bin/ffmpeg"))
        .args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "color=size=320x180:rate=24:duration=0.2",
            "-an",
            "-c:v",
            "h264_videotoolbox",
            "-allow_sw",
            "0",
            "-b:v",
            "600000",
            "-f",
            "null",
            "-",
        ])
        .output()
        .unwrap();
    probe.status.success()
}

#[test]
#[ignore = "requires bundled FFmpeg and real hardware execution"]
fn auto_and_cpu_preserve_complete_video_and_strict_one_mb_limit() {
    let runtime = runtime();
    let sources = tempfile::tempdir().unwrap();
    let input = sources.path().join("performance-source.mp4");
    fixture(&runtime, &input, 10, "640x360");
    let original = std::fs::read(&input).unwrap();
    let hardware = actual_hardware_available(&runtime);
    let engine = Engine::new(runtime.clone());
    for (profile, mode) in [
        ("auto-cold", "auto"),
        ("auto-cached", "auto"),
        ("cpu", "cpu"),
    ] {
        let outputs = tempfile::tempdir().unwrap();
        engine.import(vec![input.clone()]).unwrap();
        let started = Instant::now();
        engine
            .start(configuration(outputs.path(), "mp4", mode, Some(1_000_000)))
            .unwrap();
        let (snapshot, observed, _) = wait(&engine);
        let elapsed = started.elapsed();
        let job = &snapshot.jobs[0];
        assert_eq!(job.status, "done", "mode={mode}: {:?}", job.error);
        let encoder = serde_json::to_value(job).unwrap()["encoder"]
            .as_str()
            .unwrap()
            .to_owned();
        if mode == "cpu" || !cfg!(target_os = "macos") {
            assert_eq!(encoder, "libx264", "CPU execution used a hardware encoder");
            assert!(!observed.contains("h264_videotoolbox"));
        } else {
            assert!(
                ["libx264", "h264_videotoolbox"].contains(&encoder.as_str()),
                "automatic selected an unexpected encoder: {encoder}"
            );
        }
        let bytes = verify(
            &runtime,
            Path::new(job.output.as_ref().unwrap()),
            10.0,
            Some(1_000_000),
        );
        assert!(
            bytes >= 850_000,
            "mode={mode} only used {bytes} bytes of 1 MB quality budget"
        );
        assert_eq!(job.output_size, Some(bytes));
        assert_eq!(job.target_bytes, Some(1_000_000));
        assert_eq!(
            std::fs::read_dir(outputs.path()).unwrap().count(),
            1,
            "temporary outputs or pass logs leaked"
        );
        assert_eq!(std::fs::read(&input).unwrap(), original);
        let note = serde_json::to_value(job).unwrap()["performance_note"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        eprintln!("performance acceptance: profile={profile} mode={mode} hardware_available={hardware} encoder={encoder} observed={observed:?} bytes={bytes} elapsed_ms={} note={note}", elapsed.as_millis());
        engine.remove(job.id.clone()).unwrap();
    }
    engine.shutdown();
}

#[test]
#[ignore = "requires bundled FFmpeg and real adaptive workers"]
fn automatic_workers_finish_isolated_jobs_with_bounded_parallelism() {
    let runtime = runtime();
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("source.mp4");
    fixture(&runtime, &input, 5, "320x180");
    let mut inputs = vec![input.clone()];
    for index in 1..4 {
        let copy = sources.path().join(format!("source-{index}.mp4"));
        std::fs::copy(&input, &copy).unwrap();
        inputs.push(copy);
    }
    let engine = Engine::new(runtime.clone());
    engine.import(inputs).unwrap();
    engine
        .start(configuration(outputs.path(), "mp4", "auto", Some(150_000)))
        .unwrap();
    let (snapshot, encoders, peak) = wait(&engine);
    engine.shutdown();
    let cpus = thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    assert!(
        peak <= cpus.min(4),
        "{peak} workers oversubscribed {cpus} logical CPUs"
    );
    let mut paths = BTreeSet::new();
    for job in &snapshot.jobs {
        assert_eq!(job.status, "done", "{:?}", job.error);
        let output = PathBuf::from(job.output.as_ref().unwrap());
        assert!(paths.insert(output.clone()), "jobs collided on output path");
        verify(&runtime, &output, 5.0, Some(150_000));
    }
    assert_eq!(paths.len(), 4);
    assert_eq!(
        std::fs::read_dir(outputs.path()).unwrap().count(),
        4,
        "temporary outputs or pass logs leaked"
    );
    eprintln!("automatic queue acceptance: jobs=4 peak_workers={peak} logical_cpus={cpus} encoders={encoders:?}");
}

#[test]
#[ignore = "requires bundled FFmpeg and real cancellation"]
fn automatic_workers_cancel_and_clean_every_private_artifact() {
    let runtime = runtime();
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("cancel-source.mp4");
    fixture(&runtime, &input, 16, "640x360");
    let original = std::fs::read(&input).unwrap();
    let mut inputs = vec![input.clone()];
    for index in 1..4 {
        let copy = sources.path().join(format!("cancel-{index}.mp4"));
        std::fs::copy(&input, &copy).unwrap();
        inputs.push(copy);
    }
    let engine = Engine::new(runtime);
    engine.import(inputs).unwrap();
    // VP9 is intentionally substantial enough to cancel while jobs own live encoders.
    engine
        .start(configuration(
            outputs.path(),
            "webm",
            "auto",
            Some(1_000_000),
        ))
        .unwrap();
    let began = Instant::now();
    loop {
        let snapshot = engine.snapshot();
        if snapshot
            .jobs
            .iter()
            .any(|j| j.status == "running" && j.stage.as_deref().is_some_and(|s| s != "Analysing"))
        {
            break;
        }
        assert!(
            began.elapsed() < Duration::from_secs(15),
            "no real encoder started"
        );
        thread::sleep(Duration::from_millis(10));
    }
    engine.cancel(None).unwrap();
    let (snapshot, _, _) = wait(&engine);
    engine.shutdown();
    assert!(
        snapshot.jobs.iter().all(|j| j.status == "cancelled"),
        "{:?}",
        snapshot.jobs
    );
    assert!(snapshot.jobs.iter().all(|j| j.output.is_none()));
    assert_eq!(
        std::fs::read_dir(outputs.path()).unwrap().count(),
        0,
        "cancellation leaked temporary output or pass logs"
    );
    assert_eq!(std::fs::read(input).unwrap(), original);
}

#[cfg(unix)]
#[test]
#[ignore = "requires bundled FFmpeg; injects only hardware-session failure"]
fn unavailable_hardware_falls_back_to_real_cpu_and_invalid_input_stays_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let real_runtime = runtime();
    let isolated_runtime = tempfile::tempdir().unwrap();
    let bin = isolated_runtime.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let quoted = |path: &Path| format!("'{}'", path.to_string_lossy().replace('\'', "'\"'\"'"));
    // Failure injection only: every software encode and metadata probe is the real runtime.
    let converter = bin.join("ffmpeg");
    let hardware_failure = isolated_runtime.path().join("hardware-file-encode-failed");
    std::fs::write(&converter, format!(
        "#!/bin/sh\nhardware=0\nfile_encode=0\ncalibration=0\nfor argument in \"$@\"; do\n  case \"$argument\" in\n    h264_videotoolbox) hardware=1 ;;\n    *.mp4|*.mkv|*.mov) file_encode=1 ;;\n    comment=nova-performance-calibration) calibration=1 ;;\n  esac\ndone\nif [ \"$hardware\" = 1 ] && [ \"$file_encode\" = 1 ] && [ \"$calibration\" = 0 ]; then\n  echo 'Injected unavailable VideoToolbox file session' >&2\n  echo attempted > {}\n  exit 1\nfi\n# Controlled timing ensures the available hardware wins the calibration sample.\nif [ \"$calibration\" = 1 ] && [ \"$hardware\" = 0 ]; then\n  /bin/sleep 0.25\nfi\nexec {} \"$@\"\n", quoted(&hardware_failure), quoted(&real_runtime.join("bin/ffmpeg"))
    )).unwrap();
    let probe = bin.join("ffprobe");
    std::fs::write(
        &probe,
        format!(
            "#!/bin/sh\nexec {} \"$@\"\n",
            quoted(&real_runtime.join("bin/ffprobe"))
        ),
    )
    .unwrap();
    for executable in [&converter, &probe] {
        std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let sources = tempfile::tempdir().unwrap();
    let input = sources.path().join("fallback.mp4");
    fixture(&real_runtime, &input, 5, "320x180");
    let original = std::fs::read(&input).unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let engine = Engine::new(isolated_runtime.path().into());
    engine.import(vec![input.clone()]).unwrap();
    engine
        .start(configuration(outputs.path(), "mp4", "auto", Some(150_000)))
        .unwrap();
    let (snapshot, _, _) = wait(&engine);
    engine.shutdown();
    let job = &snapshot.jobs[0];
    assert_eq!(
        job.status, "done",
        "real CPU fallback failed: {:?}",
        job.error
    );
    assert_eq!(serde_json::to_value(job).unwrap()["encoder"], "libx264");
    if actual_hardware_available(&real_runtime) {
        assert!(
            hardware_failure.exists(),
            "hardware probe succeeded, but no per-file hardware failure was exercised"
        );
    } else if !cfg!(target_os = "macos") {
        assert!(
            !hardware_failure.exists(),
            "non-macOS automatic mode attempted a VideoToolbox file session"
        );
    }
    let bytes = verify(
        &real_runtime,
        Path::new(job.output.as_ref().unwrap()),
        5.0,
        Some(150_000),
    );
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 1);
    assert_eq!(std::fs::read(&input).unwrap(), original);

    // A stale queue entry whose media becomes invalid must never be reported as converted.
    let failed_outputs = tempfile::tempdir().unwrap();
    let invalid = Engine::new(isolated_runtime.path().into());
    invalid.import(vec![input.clone()]).unwrap();
    std::fs::write(&input, b"corrupt media after import").unwrap();
    invalid
        .start(configuration(
            failed_outputs.path(),
            "mp4",
            "auto",
            Some(150_000),
        ))
        .unwrap();
    let (failure, _, _) = wait(&invalid);
    invalid.shutdown();
    assert_eq!(failure.jobs[0].status, "error");
    assert!(failure.jobs[0].output.is_none());
    assert!(failure.jobs[0]
        .error
        .as_ref()
        .is_some_and(|error| !error.is_empty()));
    assert_eq!(std::fs::read_dir(failed_outputs.path()).unwrap().count(), 0);
    eprintln!("hardware failure injection acceptance: real_cpu_output_bytes={bytes}; corrupted_source=error_without_output");
}

#[cfg(unix)]
fn instrumented_runtime(
    real: &Path,
    delay_calibration: bool,
) -> (tempfile::TempDir, PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let bin = directory.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let marker = directory.path().join("calibration-invocations");
    let pid = directory.path().join("calibration-process");
    let quote = |path: &Path| format!("'{}'", path.to_string_lossy().replace('\'', "'\"'\"'"));
    let delay = if delay_calibration {
        format!(
            "exec {} -readrate 0.01 \"$@\"",
            quote(&real.join("bin/ffmpeg"))
        )
    } else {
        String::new()
    };
    let script = format!(
        "#!/bin/sh\nfor argument in \"$@\"; do\n  if [ \"$argument\" = comment=nova-performance-calibration ]; then\n    echo sample >> {}\n    echo $$ > {}\n    {}\n    break\n  fi\ndone\nexec {} \"$@\"\n",
        quote(&marker), quote(&pid), delay, quote(&real.join("bin/ffmpeg"))
    );
    std::fs::write(bin.join("ffmpeg"), script).unwrap();
    std::fs::write(
        bin.join("ffprobe"),
        format!(
            "#!/bin/sh\nexec {} \"$@\"\n",
            quote(&real.join("bin/ffprobe"))
        ),
    )
    .unwrap();
    for name in ["ffmpeg", "ffprobe"] {
        std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    (directory, marker, pid)
}
#[cfg(unix)]
fn sample_count(marker: &Path) -> usize {
    std::fs::read_to_string(marker)
        .unwrap_or_default()
        .lines()
        .count()
}
#[cfg(unix)]
fn cache_json_files(directory: &Path) -> Vec<PathBuf> {
    let mut files = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

#[cfg(unix)]
#[test]
#[ignore = "requires real FFmpeg and writable isolated calibration cache"]
fn automatic_calibration_reuses_valid_cache_and_rejects_stale_or_malformed_entries() {
    let real = runtime();
    if !actual_hardware_available(&real) {
        eprintln!("calibration cache comparison requires working VideoToolbox; CPU-only host correctly has no two-strategy calibration");
        return;
    }
    let (instrumented, marker, _) = instrumented_runtime(&real, false);
    let cache = tempfile::tempdir().unwrap();
    let sources = tempfile::tempdir().unwrap();
    let input = sources.path().join("cache-source.mp4");
    fixture(&real, &input, 5, "320x180");
    let before = std::fs::read(&input).unwrap();
    let run = |target| {
        let output = tempfile::tempdir().unwrap();
        let engine =
            Engine::with_performance_cache(instrumented.path().into(), Some(cache.path().into()));
        engine.import(vec![input.clone()]).unwrap();
        engine
            .start(configuration(output.path(), "mp4", "auto", Some(target)))
            .unwrap();
        let (snapshot, _, _) = wait(&engine);
        engine.shutdown();
        assert_eq!(
            snapshot.jobs[0].status, "done",
            "{:?}",
            snapshot.jobs[0].error
        );
        verify(
            &real,
            Path::new(snapshot.jobs[0].output.as_ref().unwrap()),
            5.0,
            Some(target),
        );
        assert_eq!(
            std::fs::read_dir(output.path()).unwrap().count(),
            1,
            "calibration artifacts leaked beside output"
        );
        serde_json::to_value(&snapshot.jobs[0]).unwrap()["performance_note"]
            .as_str()
            .expect("automatic strategy must explain its measured choice")
            .to_owned()
    };
    let measured_note = run(150_000);
    assert!(
        measured_note.to_ascii_lowercase().contains("measured"),
        "{measured_note}"
    );
    let first_samples = sample_count(&marker);
    assert!(
        first_samples >= 2,
        "automatic did not actually measure CPU and hardware"
    );
    let documents = cache_json_files(cache.path());
    assert!(
        !documents.is_empty(),
        "successful calibration was not cached"
    );
    let first_records = documents
        .iter()
        .map(|path| (path.clone(), std::fs::read(path).unwrap()))
        .collect::<Vec<_>>();
    for (_, bytes) in &first_records {
        let _: serde_json::Value =
            serde_json::from_slice(bytes).expect("calibration cache must be valid JSON");
    }
    let cached_note = run(150_000);
    assert!(
        cached_note.to_ascii_lowercase().contains("cached"),
        "{cached_note}"
    );
    assert_eq!(
        sample_count(&marker),
        first_samples,
        "identical runtime and workload repeated calibration instead of reusing disk cache"
    );
    for (path, bytes) in &first_records {
        assert_eq!(
            std::fs::read(path).unwrap(),
            *bytes,
            "cache hit rewrote the measured entry"
        );
    }
    for path in &documents {
        std::fs::write(path, b"{malformed calibration cache").unwrap();
    }
    run(150_000);
    let regenerated_samples = sample_count(&marker);
    assert!(
        regenerated_samples > first_samples,
        "malformed JSON suppressed real recalibration"
    );
    for path in cache_json_files(cache.path()) {
        let _: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap())
            .expect("malformed cache was not repaired");
    }
    let mut last_samples = regenerated_samples;
    for created in [0, u64::MAX] {
        let mut record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&documents[0]).unwrap()).unwrap();
        record["created_unix_secs"] = created.into();
        std::fs::write(&documents[0], serde_json::to_vec(&record).unwrap()).unwrap();
        run(150_000);
        let now = sample_count(&marker);
        assert!(
            now > last_samples,
            "expired or future-dated cache entry suppressed recalibration"
        );
        last_samples = now;
    }
    // Runtime metadata belongs in the key, even if the executable's displayed version is unchanged.
    let executable = instrumented.path().join("bin/ffmpeg");
    let mut script = std::fs::read_to_string(&executable).unwrap();
    script.push_str("\n# runtime revision for cache invalidation\n");
    std::fs::write(executable, script).unwrap();
    run(150_000);
    let revised_runtime_samples = sample_count(&marker);
    assert!(
        revised_runtime_samples > last_samples,
        "changed runtime executable reused an outdated measurement"
    );
    run(100_000);
    assert!(
        sample_count(&marker) > revised_runtime_samples,
        "different target bitrate reused the wrong workload measurement"
    );
    assert_eq!(std::fs::read(input).unwrap(), before);
    eprintln!("calibration cache acceptance: initial_samples={first_samples} malformed_regeneration_samples={regenerated_samples} runtime_revision_samples={revised_runtime_samples} final_samples={}", sample_count(&marker));
}

#[cfg(unix)]
#[test]
#[ignore = "requires real FFmpeg and live calibration cancellation"]
fn cancelling_calibration_kills_its_real_encoder_without_output_or_cache() {
    let real = runtime();
    if !actual_hardware_available(&real) {
        eprintln!("calibration cancellation comparison requires working VideoToolbox; CPU-only host has no calibration child");
        return;
    }
    let (instrumented, marker, pid_file) = instrumented_runtime(&real, true);
    let cache = tempfile::tempdir().unwrap();
    let sources = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let input = sources.path().join("cancel-calibration.mp4");
    fixture(&real, &input, 8, "640x360");
    let original = std::fs::read(&input).unwrap();
    let engine =
        Engine::with_performance_cache(instrumented.path().into(), Some(cache.path().into()));
    engine.import(vec![input.clone()]).unwrap();
    engine
        .start(configuration(output.path(), "mp4", "auto", Some(500_000)))
        .unwrap();
    let began = Instant::now();
    loop {
        if marker.exists() && pid_file.exists() {
            let snapshot = engine.snapshot();
            assert!(
                snapshot.running,
                "calibration must belong to a running, cancellable job"
            );
            assert!(
                snapshot.jobs[0]
                    .stage
                    .as_deref()
                    .is_some_and(|stage| stage.to_ascii_lowercase().contains("calibrat")),
                "calibration stage missing: {:?}",
                snapshot.jobs[0].stage
            );
            break;
        }
        assert!(
            began.elapsed() < Duration::from_secs(15),
            "real calibration process never started"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let process = std::fs::read_to_string(pid_file).unwrap();
    let cancelled_at = Instant::now();
    engine.cancel(None).unwrap();
    let (snapshot, _, _) = wait(&engine);
    engine.shutdown();
    assert!(
        cancelled_at.elapsed() < Duration::from_secs(2),
        "cancellation waited for the calibration deadline"
    );
    assert_eq!(snapshot.jobs[0].status, "cancelled");
    assert!(snapshot.jobs[0].output.is_none());
    assert_eq!(
        std::fs::read_dir(output.path()).unwrap().count(),
        0,
        "calibration output or pass logs leaked"
    );
    assert_eq!(
        std::fs::read_dir(cache.path()).unwrap().count(),
        0,
        "cancelled measurement was cached"
    );
    let alive = Command::new("/bin/kill")
        .args(["-0", process.trim()])
        .output()
        .unwrap()
        .status
        .success();
    assert!(!alive, "cancelled FFmpeg process survived cleanup");
    assert_eq!(std::fs::read(input).unwrap(), original);
    eprintln!(
        "calibration cancellation acceptance: stopped_ms={}",
        cancelled_at.elapsed().as_millis()
    );
}

#[cfg(unix)]
#[test]
#[ignore = "requires real FFmpeg and captures actual calibration/production arguments"]
fn normal_video_preset_calibrates_the_same_budget_it_really_converts() {
    fn argument<'a>(arguments: &'a [String], option: &str) -> Option<&'a str> {
        arguments
            .windows(2)
            .find(|pair| pair[0] == option)
            .map(|pair| pair[1].as_str())
    }
    fn cpu_plan(arguments: &[String]) -> Vec<Option<String>> {
        [
            "-pass",
            "-b:v",
            "-b:a",
            "-c:a",
            "-ar",
            "-ac",
            "-preset",
            "-pix_fmt",
            "-fps_mode",
            "-enc_time_base:v",
            "-f",
        ]
        .iter()
        .map(|option| argument(arguments, option).map(str::to_owned))
        .collect()
    }
    let real = runtime();
    let hardware = actual_hardware_available(&real);
    let (instrumented, _, _) = instrumented_runtime(&real, false);
    let quote = |path: &Path| format!("'{}'", path.to_string_lossy().replace('\'', "'\"'\"'"));
    let converter = instrumented.path().join("bin/ffmpeg");
    let original_wrapper = std::fs::read_to_string(&converter).unwrap();
    let sources = tempfile::tempdir().unwrap();
    let input = sources.path().join("normal-quality.mp4");
    fixture(&real, &input, 3, "640x360");
    let input = std::fs::canonicalize(input).unwrap();
    let original = std::fs::read(&input).unwrap();
    let source_bytes = original.len() as u64;
    let ceiling = source_bytes * 65 / 100;
    let mut calibration_plans = BTreeSet::new();
    let mut production_plans = BTreeSet::new();
    for mode in ["auto", "cpu"] {
        // Capture arguments before transparently forwarding every call to real FFmpeg.
        // Separate process files avoid interleaving concurrent probe/encode arguments.
        let captures = instrumented.path().join(format!("{mode}-arguments"));
        std::fs::create_dir(&captures).unwrap();
        let capture = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}/\"$$.args\"\n{}",
            quote(&captures),
            original_wrapper.strip_prefix("#!/bin/sh\n").unwrap()
        );
        std::fs::write(&converter, capture).unwrap();
        let outputs = tempfile::tempdir().unwrap();
        let engine = Engine::with_performance_cache(instrumented.path().into(), None);
        engine.import(vec![input.clone()]).unwrap();
        engine
            .start(configuration(outputs.path(), "mp4", mode, None))
            .unwrap();
        let (snapshot, _, _) = wait(&engine);
        engine.shutdown();
        let job = &snapshot.jobs[0];
        assert_eq!(job.status, "done", "mode={mode}: {:?}", job.error);
        let bytes = verify(
            &real,
            Path::new(job.output.as_ref().unwrap()),
            3.0,
            Some(ceiling),
        );
        assert_eq!(job.size, source_bytes);
        assert_eq!(job.target_bytes, Some(ceiling));
        assert!(
            job.target_is_preset,
            "mode={mode} lost the preset ceiling origin"
        );
        assert_eq!(job.output_size, Some(bytes));
        assert_eq!(
            (job.output_width, job.output_height),
            (Some(640), Some(360))
        );
        assert!(job
            .output_frame_rate
            .is_some_and(|rate| (rate - 24.0).abs() < 0.001));
        assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 1);

        let captured_arguments = std::fs::read_dir(&captures)
            .unwrap()
            .map(|entry| {
                std::fs::read_to_string(entry.unwrap().path())
                    .unwrap()
                    .lines()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        if !cfg!(target_os = "macos") {
            assert!(
                captured_arguments
                    .iter()
                    .all(|arguments| !arguments.iter().any(|value| value == "h264_videotoolbox")),
                "mode={mode} attempted VideoToolbox on a non-macOS host"
            );
        }
        let invocations = captured_arguments
            .into_iter()
            .filter(|arguments| argument(arguments, "-i") == input.to_str())
            .filter(|arguments| {
                matches!(
                    argument(arguments, "-c:v"),
                    Some("libx264" | "h264_videotoolbox")
                )
            })
            .collect::<Vec<_>>();
        assert!(
            !invocations.is_empty(),
            "mode={mode} captured no real video encodes"
        );
        let mut cpu_passes = std::collections::BTreeMap::new();
        let mut sampled_rates = BTreeSet::new();
        let mut saw_cpu_calibration = false;
        let mut saw_hardware_calibration = false;
        for arguments in &invocations {
            let calibration = arguments
                .iter()
                .any(|value| value == "comment=nova-performance-calibration");
            let video_rate = argument(arguments, "-b:v")
                .expect("normal preset must use a planned video bitrate")
                .parse::<u64>()
                .unwrap();
            assert!(video_rate > 0);
            let audio_rate = argument(arguments, "-b:a")
                .map(|rate| rate.parse::<u64>().unwrap())
                .unwrap_or(0);
            if calibration && argument(arguments, "-pass") != Some("1") {
                sampled_rates.insert((video_rate, audio_rate));
            }
            assert!(!arguments.iter().any(|value| value == "-crf"));
            assert!(
                !arguments.iter().any(|value| {
                    [
                        "-vf",
                        "-filter:v",
                        "-filter_complex",
                        "-s",
                        "-s:v",
                        "-r",
                        "-r:v",
                    ]
                    .contains(&value.as_str())
                }),
                "mode={mode} introduced a geometry or cadence override: {arguments:?}"
            );
            assert_eq!(argument(arguments, "-fps_mode"), Some("passthrough"));
            assert_eq!(argument(arguments, "-enc_time_base:v"), Some("-1"));
            if argument(arguments, "-c:v") == Some("libx264") {
                assert_eq!(argument(arguments, "-preset"), Some("medium"));
                let pass = argument(arguments, "-pass")
                    .expect("CPU preset encoding must benchmark and execute two passes");
                assert!(["1", "2"].contains(&pass));
                let pass_log = argument(arguments, "-passlogfile")
                    .expect("CPU passes need a private shared pass log");
                cpu_passes
                    .entry(pass_log)
                    .or_insert_with(Vec::new)
                    .push(arguments);
                if calibration {
                    saw_cpu_calibration = true;
                    calibration_plans.insert(cpu_plan(arguments));
                } else {
                    production_plans.insert(cpu_plan(arguments));
                }
            } else if calibration {
                saw_hardware_calibration = true;
            }
        }
        for passes in cpu_passes.values() {
            assert_eq!(passes.len(), 2, "mode={mode} did not run both CPU passes");
            let first = passes
                .iter()
                .find(|arguments| argument(arguments, "-pass") == Some("1"))
                .expect("CPU first pass was missing");
            let second = passes
                .iter()
                .find(|arguments| argument(arguments, "-pass") == Some("2"))
                .expect("CPU second pass was missing");
            assert_eq!(argument(first, "-b:v"), argument(second, "-b:v"));
            assert!(first.iter().any(|value| value == "-an"));
            assert_eq!(argument(first, "-f"), Some("null"));
            assert!(argument(second, "-b:a").is_some());
            assert_eq!(argument(second, "-f"), Some("mp4"));
        }
        if mode == "auto" && hardware {
            assert!(
                saw_cpu_calibration,
                "actual CPU calibration was never executed"
            );
            assert!(
                saw_hardware_calibration,
                "actual hardware calibration was never executed"
            );
            assert_eq!(
                sampled_rates.len(),
                1,
                "CPU and hardware calibration used different planned video/audio rates: {sampled_rates:?}"
            );
        }
        if mode == "cpu" {
            assert!(
                !cpu_passes.is_empty(),
                "no actual CPU conversion was captured"
            );
            assert!(!saw_cpu_calibration && !saw_hardware_calibration);
        }
    }
    assert!(
        calibration_plans.is_subset(&production_plans),
        "calibration plans {calibration_plans:?} differed from actual CPU plans {production_plans:?}"
    );
    assert_eq!(std::fs::read(input).unwrap(), original);
    eprintln!("normal-preset acceptance: source_bytes={source_bytes} ceiling_65_percent={ceiling} hardware_available={hardware}; CPU calibration and production share planned two-pass rates without CRF; outputs fully decoded at 640x360/24 fps");
}
