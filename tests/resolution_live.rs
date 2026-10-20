//! Source geometry and cadence are immutable in quality and size-limit modes.
use nova_flux::engine::{Engine, Settings, Snapshot};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
    thread,
    time::{Duration, Instant},
};

fn runtime() -> PathBuf {
    std::env::var_os("NOVA_TEST_RUNTIME")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../media-runtime/runtime"))
}
fn settings(dir: &Path, format: &str, target: Option<u64>) -> Settings {
    serde_json::from_value(
        serde_json::json!({"format":format,"quality":"medium","output_dir":dir,
        "concurrency":1,"performance":"cpu","target_bytes":target,"resolution":null}),
    )
    .unwrap()
}
fn video(runtime: &Path, output: &Path, width: u32, height: u32, fps: u32) {
    // RGB testsrc retains odd dimensions, unlike subsampled testsrc2 fixtures.
    let result = Command::new(runtime.join("bin/ffmpeg"))
        .args(["-v", "error", "-nostdin", "-f", "lavfi", "-i"])
        .arg(format!(
            "testsrc=size={width}x{height}:rate={fps}:duration=1"
        ))
        .args([
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=721:sample_rate=48000:duration=1",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv444p",
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
fn finish(engine: &Engine) -> Snapshot {
    let started = Instant::now();
    loop {
        let snapshot = engine.snapshot();
        if !snapshot.running {
            return snapshot;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "conversion exceeded 60 seconds"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
fn probe(runtime: &Path, output: &Path) -> serde_json::Value {
    let result = Command::new(runtime.join("bin/ffprobe"))
        .args([
            "-v",
            "error",
            "-count_frames",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(output)
        .output()
        .unwrap();
    assert!(result.status.success());
    serde_json::from_slice(&result.stdout).unwrap()
}
fn video_stream(metadata: &serde_json::Value) -> &serde_json::Value {
    metadata["streams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["codec_type"] == "video")
        .unwrap()
}
fn convert(
    runtime: &Path,
    input: &Path,
    directory: &Path,
    format: &str,
    target: Option<u64>,
) -> PathBuf {
    let before = std::fs::read(input).unwrap();
    let engine = Engine::with_performance_cache(runtime.into(), None);
    engine.import(vec![input.into()]).unwrap();
    engine.start(settings(directory, format, target)).unwrap();
    let snapshot = finish(&engine);
    engine.shutdown();
    let job = &snapshot.jobs[0];
    assert_eq!(job.status, "done", "{:?}", job.error);
    let output = PathBuf::from(job.output.as_ref().unwrap());
    let bytes = std::fs::metadata(&output).unwrap().len();
    assert!(bytes > 0);
    if let Some(limit) = target {
        assert!(bytes <= limit);
    }
    assert_eq!(std::fs::read(input).unwrap(), before, "source was changed");
    assert_eq!(
        std::fs::read_dir(directory).unwrap().count(),
        1,
        "private pass files leaked"
    );
    output
}
fn assert_geometry_and_frames(runtime: &Path, output: &Path, width: u32, height: u32, fps: u32) {
    let metadata = probe(runtime, output);
    let stream = video_stream(&metadata);
    assert_eq!(
        (stream["width"].as_u64(), stream["height"].as_u64()),
        (Some(width as u64), Some(height as u64))
    );
    assert_eq!(
        stream["nb_read_frames"]
            .as_str()
            .unwrap()
            .parse::<u32>()
            .unwrap(),
        fps
    );
    let rate: Vec<_> = stream["avg_frame_rate"]
        .as_str()
        .unwrap()
        .split('/')
        .collect();
    let rate = rate[0].parse::<f64>().unwrap() / rate[1].parse::<f64>().unwrap();
    assert!(
        (rate - fps as f64).abs() < 0.02,
        "cadence changed to {rate}"
    );
    assert!(metadata["streams"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["codec_type"] == "audio"));
    let duration = metadata["format"]["duration"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!(
        (duration - 1.0).abs() < 0.15,
        "duration changed to {duration}"
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
}

#[test]
#[ignore = "requires real bundled FFmpeg"]
fn ordinary_and_target_video_preserve_exact_dimensions_and_high_frame_rates() {
    let runtime = runtime();
    let sources = tempfile::tempdir().unwrap();
    for (width, height, fps) in [(640, 360, 24), (360, 640, 30), (160, 90, 60)] {
        let input = sources.path().join(format!("{width}x{height}.mp4"));
        video(&runtime, &input, width, height, fps);
        for target in [None, Some(100_000)] {
            let outputs = tempfile::tempdir().unwrap();
            let output = convert(&runtime, &input, outputs.path(), "mp4", target);
            assert_geometry_and_frames(&runtime, &output, width, height, fps);
        }
    }
}

#[test]
#[ignore = "requires real bundled FFmpeg"]
fn odd_geometry_is_preserved_in_samples_ordinary_and_target_outputs() {
    let runtime = runtime();
    let sources = tempfile::tempdir().unwrap();
    let input = sources.path().join("odd.mp4");
    video(&runtime, &input, 641, 361, 24);
    for format in ["mp4", "webm"] {
        let engine = Engine::with_performance_cache(runtime.clone(), None);
        let imported = engine.import(vec![input.clone()]).unwrap();
        let outputs = tempfile::tempdir().unwrap();
        let sample = engine
            .measure_estimate(
                &imported.jobs[0],
                &settings(outputs.path(), format, None),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert!(["libx264", "libvpx-vp9"].contains(&sample.encoder.as_str()));
        assert!(sample.estimate.bytes > 0);
        engine.shutdown();
        for target in [None, Some(100_000)] {
            let outputs = tempfile::tempdir().unwrap();
            let output = convert(&runtime, &input, outputs.path(), format, target);
            assert_geometry_and_frames(&runtime, &output, 641, 361, 24);
        }
    }
}

#[test]
#[ignore = "requires real bundled FFmpeg"]
fn unsupported_odd_avi_errors_instead_of_resizing_any_pixels() {
    let runtime = runtime();
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("odd.mp4");
    video(&runtime, &input, 641, 361, 24);
    for target in [None, Some(100_000)] {
        let engine = Engine::with_performance_cache(runtime.clone(), None);
        let imported = engine.import(vec![input.clone()]).unwrap();
        let configuration = settings(outputs.path(), "avi", target);
        if target.is_none() {
            assert!(engine
                .measure_estimate(&imported.jobs[0], &configuration, &AtomicBool::new(false))
                .unwrap_err()
                .to_lowercase()
                .contains("odd"));
        }
        engine.start(configuration).unwrap();
        let snapshot = finish(&engine);
        assert_eq!(snapshot.jobs[0].status, "error");
        assert!(snapshot.jobs[0]
            .error
            .as_ref()
            .unwrap()
            .to_lowercase()
            .contains("odd"));
        assert!(snapshot.jobs[0].output.is_none());
        engine.shutdown();
    }
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 0);
}

#[test]
#[ignore = "requires real bundled FFmpeg"]
fn legacy_resize_requests_are_rejected_for_quality_and_stream_copy() {
    let runtime = runtime();
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("source.mp4");
    video(&runtime, &input, 640, 360, 24);
    let engine = Engine::with_performance_cache(runtime, None);
    engine.import(vec![input]).unwrap();
    for quality in ["low", "medium", "high", "original"] {
        let mut configuration = settings(outputs.path(), "mp4", None);
        configuration.quality = quality.into();
        configuration.resolution = Some(240);
        assert!(engine.start(configuration).is_err());
    }
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 0);
    engine.shutdown();
}

fn frame_timestamps(runtime: &Path, input: &Path) -> Vec<f64> {
    let output = Command::new(runtime.join("bin/ffprobe"))
        .args([
            "-v",
            "error",
            "-select_streams",
            "V:0",
            "-show_frames",
            "-show_entries",
            "frame=best_effort_timestamp_time",
            "-of",
            "json",
        ])
        .arg(input)
        .output()
        .unwrap();
    assert!(output.status.success());
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    metadata["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|frame| {
            frame["best_effort_timestamp_time"]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap()
        })
        .collect()
}

#[test]
#[ignore = "requires real bundled FFmpeg"]
fn variable_frame_timestamps_remain_variable_in_quality_and_target_modes() {
    let runtime = runtime();
    let sources = tempfile::tempdir().unwrap();
    let input = sources.path().join("variable.mp4");
    let created = Command::new(runtime.join("bin/ffmpeg"))
        .args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=30:duration=1",
            "-vf",
            "select=not(mod(n\\,2))+not(mod(n\\,5))",
            "-fps_mode",
            "passthrough",
            "-enc_time_base:v",
            "-1",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-crf",
            "14",
        ])
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let expected = frame_timestamps(&runtime, &input);
    assert_eq!(expected.len(), 18);
    let gaps: Vec<_> = expected.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert!(
        gaps.iter().any(|gap| (*gap - gaps[0]).abs() > 0.01),
        "fixture was not variable-rate"
    );
    for format in ["mp4", "avi"] {
        for target in [None, Some(100_000)] {
            let outputs = tempfile::tempdir().unwrap();
            let output = convert(&runtime, &input, outputs.path(), format, target);
            let actual = frame_timestamps(&runtime, &output);
            assert_eq!(
                actual.len(),
                expected.len(),
                "{format}: frame count changed"
            );
            for (expected, actual) in expected.iter().zip(actual) {
                // AVI's muxer clock has a coarser representable tick.
                let tick = if format == "avi" { 0.002 } else { 0.0001 };
                assert!(
                    (expected - actual).abs() < tick,
                    "{format}: frame timestamp changed: {expected} to {actual}"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires real bundled FFmpeg"]
fn anamorphic_display_aspect_and_quarter_turn_orientation_are_preserved() {
    let runtime = runtime();
    let sources = tempfile::tempdir().unwrap();
    let base = sources.path().join("anamorphic.mp4");
    let created = Command::new(runtime.join("bin/ffmpeg"))
        .args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=24:duration=1",
            "-vf",
            "setsar=4/3",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-crf",
            "14",
        ])
        .arg(&base)
        .output()
        .unwrap();
    assert!(created.status.success());
    for rotation in [0, 90] {
        let input = sources.path().join(format!("rotation-{rotation}.mp4"));
        let rotated = Command::new(runtime.join("bin/ffmpeg"))
            .args([
                "-v",
                "error",
                "-display_rotation:v:0",
                &rotation.to_string(),
                "-i",
            ])
            .arg(&base)
            .args(["-c", "copy"])
            .arg(&input)
            .output()
            .unwrap();
        assert!(rotated.status.success());
        let engine = Engine::with_performance_cache(runtime.clone(), None);
        let snapshot = engine.import(vec![input.clone()]).unwrap();
        let workspace = tempfile::tempdir().unwrap();
        engine
            .measure_estimate(
                &snapshot.jobs[0],
                &settings(workspace.path(), "mp4", None),
                &AtomicBool::new(false),
            )
            .unwrap();
        engine.shutdown();
        for target in [None, Some(100_000)] {
            let outputs = tempfile::tempdir().unwrap();
            let output = convert(&runtime, &input, outputs.path(), "mp4", target);
            let metadata = probe(&runtime, &output);
            let stream = video_stream(&metadata);
            let expected = if rotation == 0 {
                (320, 180, 4.0 / 3.0)
            } else {
                (180, 320, 3.0 / 4.0)
            };
            assert_eq!(
                (
                    stream["width"].as_u64().unwrap(),
                    stream["height"].as_u64().unwrap()
                ),
                (expected.0, expected.1)
            );
            let sar: Vec<_> = stream["sample_aspect_ratio"]
                .as_str()
                .unwrap()
                .split(':')
                .collect();
            let sar = sar[0].parse::<f64>().unwrap() / sar[1].parse::<f64>().unwrap();
            assert!((sar - expected.2).abs() < 0.001, "display aspect changed");
        }
    }
}

#[test]
#[ignore = "requires real bundled FFmpeg"]
fn arbitrary_rotation_is_rejected_before_reencoding_can_crop_the_picture() {
    let runtime = runtime();
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let base = sources.path().join("base.mp4");
    let input = sources.path().join("rotated-45.mp4");
    video(&runtime, &base, 320, 180, 24);
    let created = Command::new(runtime.join("bin/ffmpeg"))
        .args(["-v", "error", "-display_rotation:v:0", "45", "-i"])
        .arg(base)
        .args(["-c", "copy"])
        .arg(&input)
        .output()
        .unwrap();
    assert!(created.status.success());
    for target in [None, Some(100_000)] {
        let engine = Engine::with_performance_cache(runtime.clone(), None);
        let snapshot = engine.import(vec![input.clone()]).unwrap();
        let configuration = settings(outputs.path(), "mp4", target);
        if target.is_none() {
            let error = engine
                .measure_estimate(&snapshot.jobs[0], &configuration, &AtomicBool::new(false))
                .unwrap_err();
            assert!(error.contains("non-right-angle"));
        }
        engine.start(configuration).unwrap();
        let result = finish(&engine);
        assert_eq!(result.jobs[0].status, "error");
        assert!(result.jobs[0]
            .error
            .as_ref()
            .unwrap()
            .contains("Stream copy"));
        assert!(result.jobs[0].output.is_none());
        engine.shutdown();
    }
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 0);
    let engine = Engine::with_performance_cache(runtime.clone(), None);
    engine.import(vec![input]).unwrap();
    let mut copy = settings(outputs.path(), "mp4", None);
    copy.quality = "original".into();
    engine.start(copy).unwrap();
    let copied = finish(&engine);
    assert_eq!(copied.jobs[0].status, "done", "{:?}", copied.jobs[0].error);
    engine.shutdown();
}

#[test]
#[ignore = "requires real bundled FFmpeg and source-relative preset encoding"]
fn ordinary_video_presets_produce_verified_source_relative_reductions_without_resizing() {
    let runtime = runtime();
    let sources = tempfile::tempdir().unwrap();
    for (name, preset, crf, audio_rate) in [
        ("rich", "ultrafast", "10", "192k"),
        ("efficient", "medium", "32", "64k"),
    ] {
        let input = sources.path().join(format!("{name}.mp4"));
        let fixture = Command::new(runtime.join("bin/ffmpeg"))
            .args([
                "-v",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=640x360:rate=25:duration=4",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=617:sample_rate=48000:duration=4",
                "-c:v",
                "libx264",
                "-preset",
                preset,
                "-crf",
                crf,
                "-c:a",
                "aac",
                "-b:a",
                audio_rate,
            ])
            .arg(&input)
            .output()
            .unwrap();
        assert!(fixture.status.success());
        let source_bytes = std::fs::metadata(&input).unwrap().len();
        let original = std::fs::read(&input).unwrap();
        let mut sizes = Vec::new();
        for (quality, percent) in [("low", 40u64), ("medium", 65), ("high", 85)] {
            let outputs = tempfile::tempdir().unwrap();
            let engine = Engine::with_performance_cache(runtime.clone(), None);
            engine.import(vec![input.clone()]).unwrap();
            let mut configuration = settings(outputs.path(), "mp4", None);
            configuration.quality = quality.into();
            engine.start(configuration).unwrap();
            let snapshot = finish(&engine);
            engine.shutdown();
            let job = &snapshot.jobs[0];
            assert_eq!(job.status, "done", "{name}/{quality}: {:?}", job.error);
            let actual = std::fs::metadata(job.output.as_ref().unwrap())
                .unwrap()
                .len();
            let ceiling = source_bytes * percent / 100;
            assert!(
                actual <= ceiling,
                "{name}/{quality}: actual {actual} exceeds preset ceiling {ceiling}"
            );
            assert_eq!(job.target_bytes, Some(ceiling));
            assert!(job.target_is_preset);
            assert_eq!(
                (job.output_width, job.output_height),
                (Some(640), Some(360))
            );
            assert!((job.output_frame_rate.unwrap() - 25.0).abs() < 0.01);
            assert_eq!(
                video_stream(&probe(&runtime, Path::new(job.output.as_ref().unwrap())))
                    ["nb_read_frames"],
                "100"
            );
            assert_eq!(std::fs::read(&input).unwrap(), original);
            assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 1);
            sizes.push(actual);
            eprintln!("source-aware preset: source={name} quality={quality} bytes={source_bytes} ceiling={ceiling} actual={actual}");
        }
        assert!(
            sizes[0] < sizes[1] && sizes[1] < sizes[2],
            "preset outputs should fund progressively more quality: {sizes:?}"
        );
    }
}

#[test]
#[ignore = "requires real bundled FFmpeg and an already-minimal source"]
fn an_already_minimal_source_fails_clearly_instead_of_publishing_an_enlarged_preset() {
    let runtime = runtime();
    let sources = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let input = sources.path().join("minimal.mp4");
    let fixture = Command::new(runtime.join("bin/ffmpeg"))
        .args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "color=size=160x90:rate=12:duration=1",
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "45",
        ])
        .arg(&input)
        .output()
        .unwrap();
    assert!(fixture.status.success());
    let original = std::fs::read(&input).unwrap();
    let engine = Engine::with_performance_cache(runtime, None);
    engine.import(vec![input.clone()]).unwrap();
    let mut configuration = settings(outputs.path(), "mp4", None);
    configuration.quality = "low".into();
    engine.start(configuration).unwrap();
    let result = finish(&engine);
    assert_eq!(result.jobs[0].status, "error");
    let error = result.jobs[0].error.as_ref().unwrap();
    assert!(
        error.contains("source-relative preset") && error.contains("Stream copy"),
        "{error}"
    );
    assert!(result.jobs[0].output.is_none());
    assert_eq!(std::fs::read_dir(outputs.path()).unwrap().count(), 0);
    assert_eq!(std::fs::read(input).unwrap(), original);
    engine.shutdown();
}
