#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod desktop;
mod single_instance;

use nova_flux::platform;
use serde::Serialize;

#[derive(Serialize)]
struct RuntimeInfo {
    app_version: &'static str,
    ready: bool,
    version: String,
    error: Option<String>,
}

fn inspect_runtime() -> RuntimeInfo {
    let runtime = match platform::runtime_dir() {
        Ok(runtime) => runtime,
        Err(error) => {
            return RuntimeInfo {
                app_version: env!("CARGO_PKG_VERSION"),
                ready: false,
                version: String::new(),
                error: Some(error.to_string()),
            }
        }
    };
    for name in ["ffmpeg", "ffprobe"] {
        let path = platform::executable(&runtime, name);
        match platform::command(&path).arg("-version").output() {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                return RuntimeInfo {
                    app_version: env!("CARGO_PKG_VERSION"),
                    ready: false,
                    version: String::new(),
                    error: Some(format!("{name}: {}", String::from_utf8_lossy(&o.stderr))),
                }
            }
            Err(e) => {
                return RuntimeInfo {
                    app_version: env!("CARGO_PKG_VERSION"),
                    ready: false,
                    version: String::new(),
                    error: Some(format!("Cannot launch {name}: {e}")),
                }
            }
        }
    }
    let version = platform::command(platform::executable(&runtime, "ffmpeg"))
        .arg("-version")
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("FFmpeg")
                .to_string()
        })
        .unwrap_or_default();
    RuntimeInfo {
        app_version: env!("CARGO_PKG_VERSION"),
        ready: true,
        version,
        error: None,
    }
}

fn main() {
    if std::env::args().any(|arg| arg == "--diagnose") {
        let info = inspect_runtime();
        println!("{}", serde_json::to_string_pretty(&info).unwrap());
        std::process::exit(if info.ready { 0 } else { 1 });
    }
    let _instance_guard = match single_instance::acquire() {
        Ok(single_instance::Acquisition::Acquired(guard)) => guard,
        Ok(single_instance::Acquisition::AlreadyRunning) => {
            if !single_instance::activate_existing() {
                eprintln!("Nova Flux is already running. Switch to its existing window.");
            }
            return;
        }
        Err(error) => {
            eprintln!("Nova Flux cannot safely start another GUI instance: {error}");
            std::process::exit(1);
        }
    };
    let runtime = match platform::runtime_dir() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Nova Flux could not locate its bundled media runtime: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = desktop::run(runtime) {
        eprintln!("Nova Flux could not open its native desktop window: {error}");
        std::process::exit(1);
    }
}
