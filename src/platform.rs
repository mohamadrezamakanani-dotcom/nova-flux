//! Native desktop paths and operating-system integration.
use std::{
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const APPLICATION_ID: &str = "com.nova.flux";

#[derive(Clone, Copy)]
enum OperatingSystem {
    MacOs,
    Windows,
    Unix,
}
#[derive(Clone, Copy)]
enum DirectoryKind {
    Data,
    Cache,
}
#[derive(Default)]
struct UserDirectories {
    home: Option<PathBuf>,
    roaming: Option<PathBuf>,
    local: Option<PathBuf>,
    xdg_data: Option<PathBuf>,
    xdg_cache: Option<PathBuf>,
}

fn current_os() -> OperatingSystem {
    if cfg!(target_os = "macos") {
        OperatingSystem::MacOs
    } else if cfg!(target_os = "windows") {
        OperatingSystem::Windows
    } else {
        OperatingSystem::Unix
    }
}

fn absolute(path: &Option<PathBuf>) -> Option<&Path> {
    path.as_deref().filter(|path| path.is_absolute())
}

fn directory_for(
    os: OperatingSystem,
    kind: DirectoryKind,
    roots: &UserDirectories,
) -> io::Result<PathBuf> {
    let home = absolute(&roots.home);
    let root = match (os, kind) {
        (OperatingSystem::MacOs, DirectoryKind::Data) => {
            home.map(|home| home.join("Library/Application Support"))
        }
        (OperatingSystem::MacOs, DirectoryKind::Cache) => {
            home.map(|home| home.join("Library/Caches"))
        }
        (OperatingSystem::Windows, DirectoryKind::Data) => absolute(&roots.roaming)
            .or_else(|| absolute(&roots.local))
            .map(Path::to_path_buf)
            .or_else(|| home.map(|home| home.join("AppData/Roaming"))),
        (OperatingSystem::Windows, DirectoryKind::Cache) => absolute(&roots.local)
            .map(Path::to_path_buf)
            .or_else(|| home.map(|home| home.join("AppData/Local"))),
        (OperatingSystem::Unix, DirectoryKind::Data) => absolute(&roots.xdg_data)
            .map(Path::to_path_buf)
            .or_else(|| home.map(|home| home.join(".local/share"))),
        (OperatingSystem::Unix, DirectoryKind::Cache) => absolute(&roots.xdg_cache)
            .map(Path::to_path_buf)
            .or_else(|| home.map(|home| home.join(".cache"))),
    }
    .ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Cannot locate an absolute user application directory",
        )
    })?;
    let path = root.join(APPLICATION_ID);
    Ok(
        if matches!((os, kind), (OperatingSystem::Windows, DirectoryKind::Cache)) {
            path.join("cache")
        } else {
            path
        },
    )
}

fn directory_override(value: Option<OsString>, name: &str) -> io::Result<Option<PathBuf>> {
    value
        .filter(|value| !value.is_empty())
        .map(|value| {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                Ok(path)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{name} must be an absolute directory path"),
                ))
            }
        })
        .transpose()
}

fn application_directory(kind: DirectoryKind) -> io::Result<PathBuf> {
    let name = match kind {
        DirectoryKind::Data => "NOVA_DATA_DIR",
        DirectoryKind::Cache => "NOVA_CACHE_DIR",
    };
    if let Some(path) = directory_override(std::env::var_os(name), name)? {
        return Ok(path);
    }
    let roots = UserDirectories {
        home: std::env::var_os(if cfg!(target_os = "windows") {
            "USERPROFILE"
        } else {
            "HOME"
        })
        .map(PathBuf::from),
        roaming: std::env::var_os("APPDATA").map(PathBuf::from),
        local: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        xdg_data: std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        xdg_cache: std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
    };
    directory_for(current_os(), kind, &roots)
}

/// App-owned state directory. This resolves a path without creating it.
pub fn data_dir() -> io::Result<PathBuf> {
    application_directory(DirectoryKind::Data)
}

/// App-owned cache directory, distinct from roaming preferences on Windows.
pub fn cache_dir() -> io::Result<PathBuf> {
    application_directory(DirectoryKind::Cache)
}

fn bundled_runtime_for(executable: &Path, os: OperatingSystem) -> io::Result<PathBuf> {
    let parent = executable
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Application executable has no parent directory",
            )
        })?;
    if matches!(os, OperatingSystem::MacOs) {
        let contents = parent.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Application bundle has no Contents directory",
            )
        })?;
        Ok(contents.join("Resources/runtime"))
    } else {
        Ok(parent.join("runtime"))
    }
}

/// NOVA_RUNTIME overrides the bundled runtime for development and acceptance.
/// Portable Windows/Linux distributions keep runtime/ beside the executable.
pub fn runtime_dir() -> io::Result<PathBuf> {
    if let Some(value) = std::env::var_os("NOVA_RUNTIME").filter(|value| !value.is_empty()) {
        let path = PathBuf::from(value);
        return if path.is_absolute() {
            Ok(path)
        } else {
            Ok(std::env::current_dir()?.join(path))
        };
    }
    bundled_runtime_for(&std::env::current_exe()?, current_os())
}

fn executable_for(runtime: &Path, name: &str, os: OperatingSystem) -> PathBuf {
    let name =
        if matches!(os, OperatingSystem::Windows) && !name.to_ascii_lowercase().ends_with(".exe") {
            format!("{name}.exe")
        } else {
            name.to_owned()
        };
    runtime.join("bin").join(name)
}

pub fn executable(runtime: &Path, name: &str) -> PathBuf {
    executable_for(runtime, name, current_os())
}

/// Do not flash codec/inspector console windows from the native Windows GUI.
/// Explicitly piped standard streams still work for diagnostics and progress.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let command = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut command = command;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        command
    }
    #[cfg(not(target_os = "windows"))]
    {
        command
    }
}

/// Reveal a saved output through the desktop's file manager, without a shell.
/// Linux opens its containing directory through the XDG desktop handler.
pub fn reveal(path: &Path) -> io::Result<()> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Only a saved output file can be revealed",
        ));
    }
    #[cfg(target_os = "windows")]
    {
        windows::reveal(path)
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("/usr/bin/open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let path = path.canonicalize()?;
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "Output has no parent directory")
        })?;
        Command::new("xdg-open").arg(parent).spawn().map(|_| ())
    }
}

/// Close a native UI smoke run only after the desktop has had time to render.
pub fn smoke_test_duration() -> Option<Duration> {
    let cli = std::env::args_os().any(|argument| argument == "--smoke-test");
    let environment = std::env::var("NOVA_SMOKE_TEST")
        .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
    let milliseconds = std::env::var("NOVA_SMOKE_TEST_MS").ok();
    smoke_duration(cli || environment, milliseconds.as_deref())
}

fn smoke_duration(enabled: bool, milliseconds: Option<&str>) -> Option<Duration> {
    enabled.then(|| {
        Duration::from_millis(
            milliseconds
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(1_500)
                .clamp(100, 60_000),
        )
    })
}

#[cfg(any(target_os = "windows", test))]
fn windows_shell_path_units(units: &[u16]) -> io::Result<Vec<u16>> {
    // Rust canonical paths use \\?\ prefixes; Explorer's legacy PIDL parser
    // expects regular drive/UNC paths. Preserve every UTF-16 code unit.
    if units.starts_with(&[92, 92, 63, 92]) {
        if units.len() >= 8 && units[4..8] == [85, 78, 67, 92] {
            Ok([&[92, 92][..], &units[8..]].concat())
        } else if units.len() >= 7
            && units[5..7] == [58, 92]
            && ((65..=90).contains(&units[4]) || (97..=122).contains(&units[4]))
        {
            Ok(units[4..].to_vec())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "File Explorer cannot reveal this device path",
            ))
        }
    } else {
        Ok(units.to_vec())
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::{
        ffi::c_void,
        io,
        os::windows::ffi::{OsStrExt, OsStringExt},
        path::Path,
    };

    #[link(name = "ole32")]
    unsafe extern "system" {
        #[link_name = "CoInitializeEx"]
        fn co_initialize_ex(reserved: *const c_void, concurrency: u32) -> i32;
        #[link_name = "CoUninitialize"]
        fn co_uninitialize();
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        #[link_name = "ILCreateFromPathW"]
        fn il_create_from_path(path: *const u16) -> *mut c_void;
        #[link_name = "ILFree"]
        fn il_free(item: *const c_void);
        #[link_name = "SHOpenFolderAndSelectItems"]
        fn sh_open_folder_and_select_items(
            folder: *const c_void,
            count: u32,
            children: *const *const c_void,
            flags: u32,
        ) -> i32;
    }

    pub(super) fn reveal(path: &Path) -> io::Result<()> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        let original: Vec<u16> = path.as_os_str().encode_wide().collect();
        let mut wide = super::windows_shell_path_units(&original)?;
        if wide.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Output path contains a NUL character",
            ));
        }
        // Stripping a verbatim prefix must not change the selected file, e.g.
        // when an unusual path depends on trailing dots/spaces in a component.
        let shell_path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&wide));
        if shell_path.canonicalize()? != path.canonicalize()? {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "File Explorer cannot represent this extended output path safely",
            ));
        }
        wide.push(0);
        // SAFETY: Called on the requesting worker thread; the UTF-16 path is
        // NUL-terminated, PIDL stays alive through the call and is then freed.
        unsafe {
            let initialized = co_initialize_ex(std::ptr::null(), 2);
            // RPC_E_CHANGED_MODE means this thread already has a usable COM apartment.
            if initialized < 0 && initialized as u32 != 0x8001_0106 {
                return Err(io::Error::other(format!(
                    "Cannot initialize File Explorer integration (HRESULT 0x{:08x})",
                    initialized as u32
                )));
            }
            let item = il_create_from_path(wide.as_ptr());
            let result = if item.is_null() {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "File Explorer could not resolve the saved output",
                ))
            } else {
                // cidl=0 selects the item represented by the absolute PIDL.
                let result = sh_open_folder_and_select_items(item, 0, std::ptr::null(), 0);
                il_free(item);
                if result < 0 {
                    Err(io::Error::other(format!(
                        "File Explorer could not reveal the output (HRESULT 0x{:08x})",
                        result as u32
                    )))
                } else {
                    Ok(())
                }
            };
            if initialized >= 0 {
                co_uninitialize();
            }
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        std::env::temp_dir().join("nova-portable-path-fixture")
    }

    #[test]
    fn mac_paths_preserve_installed_preferences_and_cache_locations() {
        let home = root();
        let roots = UserDirectories {
            home: Some(home.clone()),
            ..Default::default()
        };
        assert_eq!(
            directory_for(OperatingSystem::MacOs, DirectoryKind::Data, &roots).unwrap(),
            home.join("Library/Application Support/com.nova.flux")
        );
        assert_eq!(
            directory_for(OperatingSystem::MacOs, DirectoryKind::Cache, &roots).unwrap(),
            home.join("Library/Caches/com.nova.flux")
        );
    }

    #[test]
    fn windows_separates_roaming_preferences_from_local_cache() {
        let roots = UserDirectories {
            roaming: Some(root().join("Roaming")),
            local: Some(root().join("Local")),
            ..Default::default()
        };
        assert_eq!(
            directory_for(OperatingSystem::Windows, DirectoryKind::Data, &roots).unwrap(),
            root().join("Roaming/com.nova.flux")
        );
        assert_eq!(
            directory_for(OperatingSystem::Windows, DirectoryKind::Cache, &roots).unwrap(),
            root().join("Local/com.nova.flux/cache")
        );
    }

    #[test]
    fn linux_uses_absolute_xdg_roots_and_ignores_relative_values() {
        let roots = UserDirectories {
            home: Some(root().join("home")),
            xdg_data: Some(root().join("data")),
            xdg_cache: Some(PathBuf::from("relative-cache")),
            ..Default::default()
        };
        assert_eq!(
            directory_for(OperatingSystem::Unix, DirectoryKind::Data, &roots).unwrap(),
            root().join("data/com.nova.flux")
        );
        assert_eq!(
            directory_for(OperatingSystem::Unix, DirectoryKind::Cache, &roots).unwrap(),
            root().join("home/.cache/com.nova.flux")
        );
    }

    #[test]
    fn missing_user_roots_fail_instead_of_using_the_working_directory() {
        for os in [
            OperatingSystem::MacOs,
            OperatingSystem::Windows,
            OperatingSystem::Unix,
        ] {
            assert!(directory_for(os, DirectoryKind::Data, &UserDirectories::default()).is_err());
        }
    }

    #[test]
    fn runtime_layout_matches_each_distribution_and_windows_suffix() {
        let mac = root().join("Nova Flux.app/Contents/MacOS/nova-flux");
        assert_eq!(
            bundled_runtime_for(&mac, OperatingSystem::MacOs).unwrap(),
            root().join("Nova Flux.app/Contents/Resources/runtime")
        );
        for os in [OperatingSystem::Windows, OperatingSystem::Unix] {
            let binary = root().join("portable/nova-flux");
            assert_eq!(
                bundled_runtime_for(&binary, os).unwrap(),
                root().join("portable/runtime")
            );
        }
        assert_eq!(
            executable_for(&root(), "ffmpeg", OperatingSystem::Windows),
            root().join("bin/ffmpeg.exe")
        );
        assert_eq!(
            executable_for(&root(), "ffprobe", OperatingSystem::Unix),
            root().join("bin/ffprobe")
        );
        assert_eq!(
            executable_for(&root(), "ffmpeg.exe", OperatingSystem::Windows),
            root().join("bin/ffmpeg.exe")
        );
    }

    #[test]
    fn smoke_test_is_opt_in_and_has_a_bounded_duration() {
        assert_eq!(smoke_duration(false, Some("1000")), None);
        assert_eq!(
            smoke_duration(true, None),
            Some(Duration::from_millis(1_500))
        );
        assert_eq!(
            smoke_duration(true, Some("0")),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            smoke_duration(true, Some("999999")),
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            smoke_duration(true, Some("invalid")),
            Some(Duration::from_millis(1_500))
        );
    }

    #[test]
    fn windows_explorer_paths_preserve_unicode_while_removing_verbatim_prefixes() {
        for (input, expected) in [
            (r"\\?\C:\folder\فایل.mp4", r"C:\folder\فایل.mp4"),
            (r"\\?\UNC\server\share\file.mp4", r"\\server\share\file.mp4"),
            (r"D:\folder\output.mp4", r"D:\folder\output.mp4"),
        ] {
            let result =
                windows_shell_path_units(&input.encode_utf16().collect::<Vec<_>>()).unwrap();
            assert_eq!(String::from_utf16(&result).unwrap(), expected);
        }
        assert!(windows_shell_path_units(
            &r"\\?\Volume{device}\output.mp4"
                .encode_utf16()
                .collect::<Vec<_>>()
        )
        .is_err());
    }
}
