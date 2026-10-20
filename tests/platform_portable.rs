//! Portable OS integration contracts; no shell fixtures or personal directories.
use nova_flux::platform;

#[test]
fn native_media_executables_resolve_inside_the_selected_runtime() {
    let directory = tempfile::tempdir().unwrap();
    for name in ["ffmpeg", "ffprobe"] {
        let filename = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        assert_eq!(
            platform::executable(directory.path(), name),
            directory.path().join("bin").join(filename)
        );
    }
}

#[test]
fn revealing_a_missing_file_fails_before_invoking_a_desktop_handler() {
    let directory = tempfile::tempdir().unwrap();
    let error = platform::reveal(&directory.path().join("removed-output.mp4")).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

#[cfg(feature = "desktop")]
#[test]
fn diagnostics_honors_the_runtime_override_without_creating_gui_state() {
    let directory = tempfile::tempdir().unwrap();
    let data = directory.path().join("isolated-state");
    let cache = directory.path().join("isolated-cache");
    let missing_runtime = directory.path().join("missing-runtime");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_nova-flux"))
        .arg("--diagnose")
        .env("NOVA_RUNTIME", &missing_runtime)
        .env("NOVA_DATA_DIR", &data)
        .env("NOVA_CACHE_DIR", &cache)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let info: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(info["ready"], false);
    assert_eq!(info["app_version"], env!("CARGO_PKG_VERSION"));
    assert!(info["error"].as_str().unwrap().contains("ffmpeg"));
    assert!(
        !data.exists(),
        "Diagnostics must bypass the GUI lock and preferences"
    );
    assert!(
        !cache.exists(),
        "Diagnostics must not construct the conversion engine"
    );
}
