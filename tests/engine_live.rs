//! Real media tests: NOVA_TEST_RUNTIME=/absolute/runtime cargo test --no-default-features --test engine_live -- --ignored --test-threads=1
use nova_flux::engine::{Engine, Settings, Snapshot};
use std::{
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

fn runtime() -> PathBuf {
    PathBuf::from(
        std::env::var_os("NOVA_TEST_RUNTIME")
            .expect("Set NOVA_TEST_RUNTIME to bundled runtime directory"),
    )
}
fn fixture(runtime: &Path, output: &Path, seconds: &str) {
    let status = Command::new(runtime.join("bin/ffmpeg"))
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=128x96:rate=12",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            seconds,
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-c:a",
            "aac",
            "-y",
        ])
        .arg(output)
        .status()
        .unwrap();
    assert!(status.success());
}
fn settings(dir: &Path, format: &str, quality: &str) -> Settings {
    Settings {
        format: format.into(),
        quality: quality.into(),
        output_dir: dir.to_string_lossy().into(),
        concurrency: 3,
        performance: "auto".into(),
        target_bytes: None,
        resolution: None,
    }
}
fn finish(engine: &Engine) -> Snapshot {
    let start = Instant::now();
    loop {
        let snapshot = engine.snapshot();
        if !snapshot.running {
            return snapshot;
        }
        if start.elapsed() > Duration::from_secs(90) {
            engine.shutdown();
            panic!("conversion timed out");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

// Catches codecs/muxers unsupported by the shipped runtime and empty/corrupt success outputs.
#[test]
#[ignore = "requires real bundled FFmpeg runtime"]
fn every_output_format_and_quality_produces_probeable_media() {
    let runtime = runtime();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("ویدیو spaces ; $(echo nope).mp4");
    let created = Command::new(runtime.join("bin/ffmpeg"))
        .args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=640x360:rate=24:duration=3",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=3",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-crf",
            "10",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
        ])
        .arg(&input)
        .status()
        .unwrap();
    assert!(created.success());
    let before = std::fs::read(&input).unwrap();
    for format in [
        "mp4", "mkv", "avi", "mov", "webm", "mp3", "wav", "aac", "ogg", "m4a", "flac",
    ] {
        for quality in ["low", "medium", "high"] {
            let engine = Engine::new(runtime.clone());
            let imported = engine.import(vec![input.clone(), input.clone()]).unwrap();
            assert_eq!(imported.jobs.len(), 1, "duplicate input");
            assert_eq!(imported.jobs[0].status, "queued");
            assert!(imported.jobs[0].duration > 0.5);
            engine.start(settings(dir.path(), format, quality)).unwrap();
            let snapshot = finish(&engine);
            let job = &snapshot.jobs[0];
            assert_eq!(job.status, "done", "{format}/{quality}: {:?}", job.error);
            assert_eq!(job.progress, 100.0);
            let output = PathBuf::from(job.output.as_ref().unwrap());
            assert!(std::fs::metadata(&output).unwrap().len() > 100);
            let data = Command::new(runtime.join("bin/ffprobe"))
                .args(["-v", "error", "-show_streams", "-of", "json"])
                .arg(output)
                .output()
                .unwrap();
            assert!(data.status.success());
            let parsed: serde_json::Value = serde_json::from_slice(&data.stdout).unwrap();
            let streams = parsed["streams"].as_array().unwrap();
            assert!(streams.iter().any(|s| s["codec_type"] == "audio"));
            let is_video = ["mp4", "mkv", "avi", "mov", "webm"].contains(&format);
            assert_eq!(streams.iter().any(|s| s["codec_type"] == "video"), is_video);
            let decoded = Command::new(runtime.join("bin/ffmpeg"))
                .args(["-v", "error", "-xerror", "-nostdin", "-i"])
                .arg(job.output.as_ref().unwrap())
                .args(["-map", "0", "-f", "null", "-"])
                .output()
                .unwrap();
            assert!(
                decoded.status.success(),
                "{format}/{quality} decode: {}",
                String::from_utf8_lossy(&decoded.stderr)
            );
            engine.shutdown();
        }
    }
    assert_eq!(
        std::fs::read(input).unwrap(),
        before,
        "source must remain untouched"
    );
}

// Catches false success in copy mode, losing corrupt files, and failed-item retry state errors.
#[test]
#[ignore = "requires real bundled FFmpeg runtime"]
fn invalid_inputs_and_copy_rejections_are_individual_errors_and_retryable() {
    let runtime = runtime();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("good.mp4");
    let corrupt = dir.path().join("broken.mp4");
    fixture(&runtime, &input, "1");
    std::fs::write(&corrupt, b"not a media file").unwrap();
    let engine = Engine::new(runtime);
    let imported = engine.import(vec![input.clone(), corrupt]).unwrap();
    assert_eq!(imported.jobs.len(), 2);
    assert_eq!(
        imported.jobs.iter().filter(|j| j.status == "error").count(),
        1
    );
    engine
        .start(settings(dir.path(), "webm", "original"))
        .unwrap();
    let failed = finish(&engine);
    let good = failed
        .jobs
        .iter()
        .find(|j| j.path == std::fs::canonicalize(&input).unwrap().to_string_lossy())
        .unwrap();
    assert_eq!(good.status, "error");
    assert!(good.output.is_none());
    engine.retry(good.id.clone()).unwrap();
    engine
        .start(settings(dir.path(), "mp4", "original"))
        .unwrap();
    let copied = finish(&engine);
    assert_eq!(
        copied.jobs.iter().find(|j| j.id == good.id).unwrap().status,
        "done"
    );
    engine.clear_finished().unwrap();
    assert!(engine.snapshot().jobs.iter().all(|j| j.status != "done"));
    engine.shutdown();
}

// Catches orphan processes/partial media after cancelling a queued batch or active worker.
#[test]
#[ignore = "requires real bundled FFmpeg runtime"]
fn cancelling_batch_cleans_temporary_outputs_and_preserves_sources() {
    let runtime = runtime();
    let input_dir = tempfile::tempdir().unwrap();
    let output_dir = tempfile::tempdir().unwrap();
    let input = input_dir.path().join("long.mp4");
    fixture(&runtime, &input, "60");
    let before = std::fs::metadata(&input).unwrap().len();
    let engine = Engine::new(runtime);
    engine.import(vec![input.clone()]).unwrap();
    engine
        .start(settings(output_dir.path(), "webm", "high"))
        .unwrap();
    thread::sleep(Duration::from_millis(100));
    engine.cancel(None).unwrap();
    let snapshot = finish(&engine);
    assert_eq!(snapshot.jobs[0].status, "cancelled");
    assert!(snapshot.jobs[0].output.is_none());
    assert_eq!(std::fs::read_dir(output_dir.path()).unwrap().count(), 0);
    assert_eq!(std::fs::metadata(input).unwrap().len(), before);
    engine.shutdown();
}

// Catches silent truncation of high-precision PCM when selecting WAV output.
#[test]
#[ignore = "requires real bundled FFmpeg runtime"]
fn wav_output_preserves_integer_and_float_sample_precision() {
    let runtime = runtime();
    let directory = tempfile::tempdir().unwrap();
    for codec in ["pcm_s24le", "pcm_s32le", "pcm_f32le", "pcm_f64le"] {
        let input = directory.path().join(format!("{codec}.wav"));
        let generated = Command::new(runtime.join("bin/ffmpeg"))
            .args([
                "-v",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=0.2",
                "-c:a",
                codec,
            ])
            .arg(&input)
            .output()
            .unwrap();
        assert!(generated.status.success());
        let engine = Engine::new(runtime.clone());
        engine.import(vec![input]).unwrap();
        engine
            .start(settings(directory.path(), "wav", "high"))
            .unwrap();
        let snapshot = finish(&engine);
        let job = &snapshot.jobs[0];
        assert_eq!(job.status, "done", "{:?}", job.error);
        let inspected = Command::new(runtime.join("bin/ffprobe"))
            .args(["-v", "error", "-show_streams", "-of", "json"])
            .arg(job.output.as_ref().unwrap())
            .output()
            .unwrap();
        let data: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
        assert_eq!(data["streams"][0]["codec_name"], codec);
        if codec == "pcm_s24le" {
            assert_eq!(data["streams"][0]["bits_per_raw_sample"], "24");
        }
        engine.shutdown();
    }
}

// Catches false truncation failures when a selected audio stream is shorter than its video container.
#[test]
#[ignore = "requires real bundled FFmpeg runtime"]
fn unequal_stream_durations_use_the_selected_stream_for_verification() {
    let runtime = runtime();
    let directory = tempfile::tempdir().unwrap();
    for (video_seconds, audio_seconds) in [("5", "1.2"), ("1.2", "5")] {
        let input = directory
            .path()
            .join(format!("v{video_seconds}-a{audio_seconds}.mp4"));
        let generated = Command::new(runtime.join("bin/ffmpeg"))
            .args(["-v", "error", "-nostdin", "-f", "lavfi", "-i"])
            .arg(format!(
                "testsrc2=size=64x64:rate=10:duration={video_seconds}"
            ))
            .args(["-f", "lavfi", "-i"])
            .arg(format!("sine=duration={audio_seconds}"))
            .args(["-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac"])
            .arg(&input)
            .output()
            .unwrap();
        assert!(generated.status.success());
        for format in ["mp3", "mp4"] {
            let engine = Engine::new(runtime.clone());
            engine.import(vec![input.clone()]).unwrap();
            engine
                .start(settings(directory.path(), format, "medium"))
                .unwrap();
            let snapshot = finish(&engine);
            assert_eq!(
                snapshot.jobs[0].status, "done",
                "{video_seconds}/{audio_seconds}/{format}: {:?}",
                snapshot.jobs[0].error
            );
            let output = snapshot.jobs[0].output.as_ref().unwrap();
            let inspected = Command::new(runtime.join("bin/ffprobe"))
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
            let data: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
            let actual: f64 = data["format"]["duration"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap();
            let expected = if format == "mp3" {
                audio_seconds.parse::<f64>().unwrap()
            } else {
                5.0
            };
            assert!(
                (actual - expected).abs() < 0.25,
                "Expected {expected}, got {actual}"
            );
            engine.shutdown();
        }
    }
}

// Catches clearing failed/cancelled jobs and losing the user's opportunity to retry them.
#[test]
#[ignore = "requires real bundled FFmpeg runtime"]
fn clear_completed_keeps_failed_and_cancelled_jobs() {
    let runtime = runtime();
    let dir = tempfile::tempdir().unwrap();
    let good = dir.path().join("good.mp4");
    let queued = dir.path().join("waiting.mp4");
    let broken = dir.path().join("broken.mp4");
    fixture(&runtime, &good, "0.2");
    std::fs::copy(&good, &queued).unwrap();
    std::fs::write(&broken, b"invalid media").unwrap();
    let engine = Engine::new(runtime);
    let snapshot = engine.import(vec![good, queued, broken]).unwrap();
    engine.cancel(Some(snapshot.jobs[1].id.clone())).unwrap();
    engine.start(settings(dir.path(), "mp4", "medium")).unwrap();
    assert_eq!(
        finish(&engine)
            .jobs
            .iter()
            .filter(|j| j.status == "done")
            .count(),
        1
    );
    engine.clear_finished().unwrap();
    let snapshot = engine.snapshot();
    assert_eq!(snapshot.jobs.len(), 2);
    assert!(snapshot.jobs.iter().any(|j| j.status == "error"));
    assert!(snapshot.jobs.iter().any(|j| j.status == "cancelled"));
    engine.shutdown();
}
