//! Boundary regressions using the same public engine API used by the desktop.
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
fn ff(runtime: &Path, args: &[&str], out: &Path) {
    let r = Command::new(runtime.join("bin/ffmpeg"))
        .args(["-v", "error", "-nostdin"])
        .args(args)
        .arg(out)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
}
fn config(dir: &Path, fmt: &str, quality: &str, n: usize) -> Settings {
    Settings {
        format: fmt.into(),
        quality: quality.into(),
        output_dir: dir.to_string_lossy().into(),
        concurrency: n,
        performance: "auto".into(),
        target_bytes: None,
        resolution: None,
    }
}
fn wait(e: &Engine) -> Snapshot {
    let limit = Instant::now();
    while e.snapshot().running {
        assert!(limit.elapsed() < Duration::from_secs(90));
        thread::sleep(Duration::from_millis(20));
    }
    e.snapshot()
}
#[test]
#[ignore = "requires bundled FFmpeg"]
fn audio_only_and_silent_video_have_correct_stream_contracts() {
    let r = runtime();
    let d = tempfile::tempdir().unwrap();
    let audio = d.path().join("voice.wma");
    let silent = d.path().join("silent.mp4");
    ff(
        &r,
        &["-f", "lavfi", "-i", "sine=duration=0.8", "-c:a", "wmav2"],
        &audio,
    );
    ff(
        &r,
        &[
            "-f",
            "lavfi",
            "-i",
            "color=size=64x64:duration=0.8",
            "-an",
            "-c:v",
            "libx264",
        ],
        &silent,
    );
    let e = Engine::new(r.clone());
    e.import(vec![audio.clone(), silent]).unwrap();
    e.start(config(d.path(), "mp3", "medium", 2)).unwrap();
    let s = wait(&e);
    let a = s.jobs.iter().find(|j| j.name == "voice.wma").unwrap();
    assert_eq!(a.status, "done", "{:?}", a.error);
    assert!(a.video.is_none());
    assert_eq!(
        s.jobs
            .iter()
            .find(|j| j.name == "silent.mp4")
            .unwrap()
            .status,
        "error"
    );
    let e2 = Engine::new(r);
    e2.import(vec![audio]).unwrap();
    e2.start(config(d.path(), "mp4", "medium", 1)).unwrap();
    assert_eq!(wait(&e2).jobs[0].status, "error");
}
#[test]
#[ignore = "requires bundled FFmpeg"]
fn recursive_import_skips_symlinks_and_deduplicates_selected_paths() {
    let r = runtime();
    let d = tempfile::tempdir().unwrap();
    let nested = d.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    let a = nested.join("فارسی audio.flac");
    ff(
        &r,
        &["-f", "lavfi", "-i", "sine=duration=0.5", "-c:a", "flac"],
        &a,
    );
    std::fs::write(nested.join("note.txt"), "not media").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(d.path(), nested.join("loop")).unwrap();
    let e = Engine::new(r);
    let s = e.import(vec![d.path().into(), a.clone()]).unwrap();
    assert_eq!(s.jobs.len(), 1);
    assert_eq!(s.jobs[0].status, "queued");
    assert_eq!(e.import(vec![a]).unwrap().jobs.len(), 1);
}
#[test]
#[ignore = "requires bundled FFmpeg"]
fn cancelling_one_waiting_job_preserves_its_neighbours_and_worker_bound() {
    let r = runtime();
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let a = src.path().join("one.mp4");
    let b = src.path().join("two.mp4");
    let c = src.path().join("three.mp4");
    ff(
        &r,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=12:duration=4",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
        ],
        &a,
    );
    std::fs::copy(&a, &b).unwrap();
    std::fs::copy(&a, &c).unwrap();
    let e = Engine::new(r);
    let s = e.import(vec![a, b, c]).unwrap();
    let cancel_id = s.jobs[1].id.clone();
    e.start(config(out.path(), "webm", "high", 1)).unwrap();
    e.cancel(Some(cancel_id.clone())).unwrap();
    let start = Instant::now();
    while e.snapshot().running {
        let s = e.snapshot();
        assert!(s.jobs.iter().filter(|j| j.status == "running").count() <= 1);
        assert!(start.elapsed() < Duration::from_secs(60));
        thread::sleep(Duration::from_millis(15));
    }
    let s = e.snapshot();
    assert_eq!(
        s.jobs.iter().filter(|j| j.status == "done").count(),
        2,
        "{:?}",
        s.jobs
    );
    assert_eq!(
        s.jobs.iter().find(|j| j.id == cancel_id).unwrap().status,
        "cancelled"
    );
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 2);
}
