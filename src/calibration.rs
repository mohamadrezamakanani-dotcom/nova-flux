//! Bounded, on-device comparisons for Automatic mode. These are short samples,
//! not a claim that a global optimum exists for every source or machine load.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const SCHEMA: u32 = 1;
const MAX_AGE: u64 = 7 * 24 * 60 * 60;
const MAX_CALIBRATION: Duration = Duration::from_secs(6);

pub(crate) struct Workload<'a> {
    pub input: &'a Path,
    pub class: String,
    pub format: &'a str,
    pub sample_seconds: f64,
    pub threads: usize,
    pub expect_audio: bool,
    /// One pass for ordinary quality, two passes for a strict size budget.
    pub cpu_arguments: Vec<Vec<String>>,
    pub hardware_arguments: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Entry {
    schema: u32,
    key: String,
    created_unix_secs: u64,
    hardware: bool,
    cpu_ms: Option<u64>,
    hardware_ms: Option<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct Selection {
    pub hardware: bool,
    pub cached: bool,
    pub cpu_ms: Option<u64>,
    pub hardware_ms: Option<u64>,
}
impl Selection {
    pub fn description(&self) -> String {
        let choice = if self.hardware {
            "Apple hardware"
        } else {
            "CPU"
        };
        let origin = if self.cached {
            "cached calibration"
        } else {
            "measured calibration"
        };
        let timings = match (self.cpu_ms, self.hardware_ms) {
            (Some(cpu), Some(hardware)) => format!("; CPU {cpu} ms, Apple hardware {hardware} ms"),
            (Some(cpu), None) => format!("; CPU {cpu} ms; hardware sample unavailable"),
            (None, Some(hardware)) => {
                format!("; Apple hardware {hardware} ms; CPU sample exceeded the calibration limit")
            }
            _ => "; sample comparison unavailable; safe CPU fallback".into(),
        };
        format!("Automatic: {choice}; {origin}{timings}")
    }
}

pub(crate) struct Calibration {
    directory: Option<PathBuf>,
    fingerprint: String,
    // Serialize calibration only; real encodes run concurrently after selection.
    entries: Mutex<HashMap<String, Entry>>,
}

pub(crate) fn configured_directory() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os("NOVAPERFORMANCECACHE") {
        if ["off", "disabled"].iter().any(|word| value == *word) {
            return None;
        }
        return Some(PathBuf::from(value));
    }
    if std::env::var_os("NOVA_TEST_RUNTIME").is_some() {
        return None;
    }
    crate::platform::cache_dir()
        .ok()
        .map(|directory| directory.join("performance"))
}

impl Calibration {
    pub fn new(runtime: &Path, directory: Option<PathBuf>) -> Self {
        Self {
            directory,
            fingerprint: fingerprint(runtime),
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn select(
        &self,
        runtime: &Path,
        workload: &Workload<'_>,
        temporary_parent: &Path,
        closed: &AtomicBool,
        token: &AtomicBool,
    ) -> Result<Selection, ()> {
        self.select_with_deadline(runtime, workload, temporary_parent, closed, token, None)
    }

    pub(crate) fn select_before(
        &self,
        runtime: &Path,
        workload: &Workload<'_>,
        temporary_parent: &Path,
        closed: &AtomicBool,
        token: &AtomicBool,
        deadline: Instant,
    ) -> Result<Selection, ()> {
        self.select_with_deadline(
            runtime,
            workload,
            temporary_parent,
            closed,
            token,
            Some(deadline),
        )
    }

    fn select_with_deadline(
        &self,
        runtime: &Path,
        workload: &Workload<'_>,
        temporary_parent: &Path,
        closed: &AtomicBool,
        token: &AtomicBool,
        deadline: Option<Instant>,
    ) -> Result<Selection, ()> {
        let cancelled = || closed.load(Ordering::Acquire) || token.load(Ordering::Acquire);
        // A waiting worker remains cancellable while another class is measured.
        let mut entries = loop {
            if cancelled() || deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Err(());
            }
            match self.entries.try_lock() {
                Ok(guard) => break guard,
                Err(std::sync::TryLockError::Poisoned(error)) => break error.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(10))
                }
            }
        };
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.fingerprint.hash(&mut hasher);
        workload.class.hash(&mut hasher);
        let key = format!("{:016x}", hasher.finish());
        let now = unix_time();
        if let Some(entry) = entries
            .get(&key)
            .filter(|entry| valid_entry(entry, &key, now))
        {
            return Ok(selection(entry, true));
        }
        if let Some(entry) = self.read_entry(&key, now) {
            let result = selection(&entry, true);
            entries.insert(key, entry);
            return Ok(result);
        }
        let workspace = tempfile::Builder::new()
            .prefix(".nova-calibration-")
            .tempdir_in(temporary_parent);
        let Ok(workspace) = workspace else {
            return Ok(Selection {
                hardware: false,
                cached: false,
                cpu_ms: None,
                hardware_ms: None,
            });
        };
        // Production waiting time remains cancellable without consuming its
        // six-second trial budget. Previews additionally keep their caller's
        // deadline across both the mutex wait and the trial itself.
        let deadline = deadline
            .unwrap_or_else(|| Instant::now() + MAX_CALIBRATION)
            .min(Instant::now() + MAX_CALIBRATION);
        // Hardware first leaves a known fast valid candidate if a very large
        // software sample consumes the rest of this bounded comparison.
        let mut hardware_ms = trial(
            runtime,
            workload,
            workspace.path(),
            true,
            0,
            deadline,
            &cancelled,
        );
        if cancelled() {
            return Err(());
        }
        let mut cpu_ms = trial(
            runtime,
            workload,
            workspace.path(),
            false,
            0,
            deadline,
            &cancelled,
        );
        if cancelled() {
            return Err(());
        }
        // Repeat close, cheap measurements in reverse order to reduce startup
        // and ordering noise, without extending the global deadline.
        if let (Some(cpu), Some(hardware)) = (cpu_ms, hardware_ms) {
            let difference = cpu.abs_diff(hardware) as f64 / cpu.max(hardware).max(1) as f64;
            if difference < 0.25
                && deadline.saturating_duration_since(Instant::now()) > Duration::from_secs(3)
            {
                if let Some(second) = trial(
                    runtime,
                    workload,
                    workspace.path(),
                    false,
                    1,
                    deadline,
                    &cancelled,
                ) {
                    cpu_ms = Some((cpu + second) / 2);
                }
                if cancelled() {
                    return Err(());
                }
                if let Some(second) = trial(
                    runtime,
                    workload,
                    workspace.path(),
                    true,
                    1,
                    deadline,
                    &cancelled,
                ) {
                    hardware_ms = Some((hardware + second) / 2);
                }
            }
        }
        if cancelled() {
            return Err(());
        }
        let hardware = match (cpu_ms, hardware_ms) {
            // Prefer CPU when the measured difference is within 5% timing noise.
            (Some(cpu), Some(hardware)) => (hardware as f64) < cpu as f64 * 0.95,
            (None, Some(_)) => true,
            _ => false,
        };
        let entry = Entry {
            schema: SCHEMA,
            key: key.clone(),
            created_unix_secs: now,
            hardware,
            cpu_ms,
            hardware_ms,
        };
        let result = selection(&entry, false);
        if cpu_ms.is_some() || hardware_ms.is_some() {
            self.write_entry(&entry);
            if entries.len() >= 128 {
                entries.clear();
            }
            entries.insert(key, entry);
        }
        Ok(result)
    }

    fn read_entry(&self, key: &str, now: u64) -> Option<Entry> {
        let path = self
            .directory
            .as_ref()?
            .join(format!("v{SCHEMA}-{key}.json"));
        if std::fs::metadata(&path).ok()?.len() > 16_384 {
            return None;
        }
        let entry: Entry = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        valid_entry(&entry, key, now).then_some(entry)
    }
    fn write_entry(&self, entry: &Entry) {
        let Some(directory) = &self.directory else {
            return;
        };
        let save = || -> Result<(), Box<dyn std::error::Error>> {
            use std::io::Write;
            std::fs::create_dir_all(directory)?;
            let mut file = tempfile::Builder::new()
                .prefix(".nova-cache-")
                .tempfile_in(directory)?;
            file.write_all(&serde_json::to_vec(entry)?)?;
            file.persist(directory.join(format!("v{SCHEMA}-{}.json", entry.key)))?;
            Ok(())
        };
        let _ = save(); // A read-only/full cache must never prevent conversion.
    }
}

fn selection(entry: &Entry, cached: bool) -> Selection {
    Selection {
        hardware: entry.hardware,
        cached,
        cpu_ms: entry.cpu_ms,
        hardware_ms: entry.hardware_ms,
    }
}
fn valid_entry(entry: &Entry, key: &str, now: u64) -> bool {
    entry.schema == SCHEMA
        && entry.key == key
        && entry.created_unix_secs <= now
        && now.saturating_sub(entry.created_unix_secs) <= MAX_AGE
        && entry
            .cpu_ms
            .into_iter()
            .chain(entry.hardware_ms)
            .all(|ms| ms > 0 && ms <= 6_000)
        && if entry.hardware {
            entry.hardware_ms.is_some()
        } else {
            entry.cpu_ms.is_some()
        }
}
fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn fingerprint(runtime: &Path) -> String {
    let mut value = format!(
        "calibration-{SCHEMA};app-{};{};{};cores-{};",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH,
        std::env::consts::OS,
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    );
    #[cfg(target_os = "macos")]
    for (program, arguments) in [
        (
            "/usr/sbin/sysctl",
            vec!["-n", "hw.model", "machdep.cpu.brand_string", "hw.memsize"],
        ),
        ("/usr/bin/sw_vers", vec!["-productVersion"]),
    ] {
        if let Ok(output) = crate::platform::command(program).args(arguments).output() {
            if output.status.success() {
                value.push_str(&String::from_utf8_lossy(&output.stdout));
            }
        }
    }
    value.push_str(
        &std::fs::canonicalize(runtime)
            .unwrap_or_else(|_| runtime.to_path_buf())
            .to_string_lossy(),
    );
    // Runtime code changes invalidate decisions, including codec libraries.
    for directory in [runtime.join("bin"), runtime.join("lib")] {
        let mut paths = std::fs::read_dir(directory)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            if let Ok(metadata) = std::fs::metadata(&path) {
                value.push_str(&format!(
                    ";{}:{}:{}",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    metadata.len(),
                    metadata
                        .modified()
                        .ok()
                        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                        .map(|time| time.as_nanos())
                        .unwrap_or(0)
                ));
            }
        }
    }
    value
}

fn trial(
    runtime: &Path,
    workload: &Workload<'_>,
    workspace: &Path,
    hardware: bool,
    trial_number: usize,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Option<u64> {
    let label = if hardware { "hardware" } else { "cpu" };
    let output = workspace.join(format!("{label}-{trial_number}.{}", workload.format));
    let pass_log = workspace.join(format!("pass-{trial_number}"));
    let passes = if hardware {
        std::slice::from_ref(&workload.hardware_arguments)
    } else {
        &workload.cpu_arguments
    };
    let started = Instant::now();
    let threads = workload.threads.max(1).to_string();
    for (index, arguments) in passes.iter().enumerate() {
        let mut command = crate::platform::command(crate::platform::executable(runtime, "ffmpeg"));
        command
            .args([
                "-hide_banner",
                "-v",
                "error",
                "-nostdin",
                "-protocol_whitelist",
                "file,pipe",
                "-threads",
                &threads,
                "-i",
            ])
            .arg(workload.input)
            .args([
                "-t",
                &format!("{:.3}", workload.sample_seconds),
                "-threads",
                &threads,
                "-filter_threads",
                &threads,
                "-filter_complex_threads",
                &threads,
            ])
            .args(arguments)
            .args([
                "-map_metadata",
                "-1",
                "-map_chapters",
                "-1",
                "-metadata",
                "comment=nova-performance-calibration",
            ]);
        if passes.len() == 2 {
            command
                .args(["-pass", &(index + 1).to_string(), "-passlogfile"])
                .arg(&pass_log);
        }
        command.arg("-y").arg(if index + 1 == passes.len() {
            output.as_path()
        } else {
            Path::new("-")
        });
        run(&mut command, false, deadline, cancelled)?;
    }
    let elapsed = started.elapsed().as_millis().max(1) as u64;
    let mut probe = crate::platform::command(crate::platform::executable(runtime, "ffprobe"));
    probe
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-show_entries",
            "stream=codec_type,codec_name,duration:format=duration",
            "-of",
            "json",
        ])
        .arg(&output);
    let data = run(&mut probe, true, deadline, cancelled)?;
    let metadata: serde_json::Value = serde_json::from_slice(&data).ok()?;
    let streams = metadata["streams"].as_array()?;
    if !streams
        .iter()
        .any(|stream| stream["codec_type"] == "video" && stream["codec_name"] == "h264")
        || workload.expect_audio && !streams.iter().any(|stream| stream["codec_type"] == "audio")
    {
        return None;
    }
    let duration = metadata["format"]["duration"]
        .as_str()?
        .parse::<f64>()
        .ok()?;
    if !duration.is_finite()
        || duration < workload.sample_seconds * 0.75
        || duration > workload.sample_seconds + 0.5
    {
        return None;
    }
    if std::fs::metadata(output).ok()?.len() == 0 {
        return None;
    }
    Some(elapsed)
}

fn run(
    command: &mut std::process::Command,
    capture: bool,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Option<Vec<u8>> {
    if cancelled() || Instant::now() >= deadline {
        return None;
    }
    command
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command.stdout(if capture {
        std::process::Stdio::piped()
    } else {
        std::process::Stdio::null()
    });
    let mut child = command.spawn().ok()?;
    let reader = child.stdout.take().map(|mut pipe| {
        std::thread::spawn(move || {
            use std::io::Read;
            let mut retained = Vec::new();
            let mut buffer = [0u8; 4096];
            while let Ok(length) = pipe.read(&mut buffer) {
                if length == 0 {
                    break;
                }
                let keep = length.min(16_384usize.saturating_sub(retained.len()));
                retained.extend_from_slice(&buffer[..keep]);
            }
            retained
        })
    });
    let success = loop {
        if cancelled() || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break false;
        }
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let output = reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    success.then_some(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_rejects_expired_future_wrong_version_or_unmeasured_decisions() {
        let entry = Entry {
            schema: SCHEMA,
            key: "test".into(),
            created_unix_secs: 100,
            hardware: false,
            cpu_ms: Some(100),
            hardware_ms: Some(120),
        };
        assert!(valid_entry(&entry, "test", 101));
        assert!(!valid_entry(&entry, "other", 101));
        assert!(!valid_entry(&entry, "test", 99));
        assert!(!valid_entry(&entry, "test", MAX_AGE + 101));
        assert!(!valid_entry(
            &Entry {
                schema: 999,
                ..entry.clone()
            },
            "test",
            101
        ));
        assert!(!valid_entry(
            &Entry {
                cpu_ms: None,
                ..entry
            },
            "test",
            101
        ));
    }
}
