use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[path = "preview.rs"]
mod preview;
pub use preview::MeasuredEstimate;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub format: String,
    pub quality: String,
    pub output_dir: String,
    pub concurrency: usize,
    #[serde(default = "default_performance")]
    pub performance: String,
    #[serde(default)]
    pub target_bytes: Option<u64>,
    /// Legacy resize setting. Video output rejects Some; dimensions always stay unchanged.
    #[serde(default)]
    pub resolution: Option<u32>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Job {
    pub id: String,
    pub path: String,
    pub name: String,
    pub size: u64,
    /// Local metadata identity used to invalidate measurements when a source changes.
    pub source_revision: String,
    pub duration: f64,
    pub video: Option<String>,
    pub audio: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<f64>,
    pub channels: Option<u32>,
    pub sample_rate: Option<u32>,
    pub sample_format: Option<String>,
    pub bits_per_sample: Option<u32>,
    pub video_duration: Option<f64>,
    pub audio_duration: Option<f64>,
    pub video_bitrate: Option<u64>,
    pub audio_bitrate: Option<u64>,
    pub status: String,
    pub progress: f64,
    pub output: Option<String>,
    pub error: Option<String>,
    pub output_size: Option<u64>,
    pub output_width: Option<u32>,
    pub output_height: Option<u32>,
    pub output_duration: Option<f64>,
    pub output_frame_rate: Option<f64>,
    pub target_bytes: Option<u64>,
    /// True when target_bytes is the selected source-relative video preset ceiling.
    pub target_is_preset: bool,
    pub stage: Option<String>,
    pub encoder: Option<String>,
    pub performance_note: Option<String>,
}
fn compression_profile(settings: &Settings, source_bytes: u64) -> Result<(Settings, bool), String> {
    let mut profile = settings.clone();
    let preset = settings.target_bytes.is_none()
        && settings.quality != "original"
        && VIDEO_FORMATS.contains(&settings.format.as_str());
    if preset {
        profile.target_bytes = Some(crate::output_profile::source_preset_limit(source_bytes, settings)
            .ok_or("This source is too small for a smaller video preset at unchanged dimensions. Choose Stream copy or another source.")?);
    }
    Ok((profile, preset))
}

fn default_performance() -> String {
    "auto".into()
}
#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub jobs: Vec<Job>,
    pub running: bool,
    pub warnings: Vec<String>,
}
#[derive(Default)]
struct Media {
    duration: f64,
    video_duration: Option<f64>,
    audio_duration: Option<f64>,
    video: Option<String>,
    audio: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    channels: Option<u32>,
    sample_rate: Option<u32>,
    sample_format: Option<String>,
    bits_per_sample: Option<u32>,
    frame_rate: Option<f64>,
    video_bitrate: Option<u64>,
    audio_bitrate: Option<u64>,
    video_frames: Option<u64>,
    sample_aspect_ratio: Option<f64>,
    rotation: f64,
}
const VIDEO_FORMATS: &[&str] = &["mp4", "mkv", "avi", "mov", "webm"];
const AUDIO_FORMATS: &[&str] = &["mp3", "wav", "aac", "ogg", "m4a", "flac"];

fn validate_settings(s: &Settings) -> Result<PathBuf, String> {
    if !VIDEO_FORMATS.contains(&s.format.as_str()) && !AUDIO_FORMATS.contains(&s.format.as_str()) {
        return Err("Unsupported output format".into());
    }
    if !["low", "medium", "high", "original"].contains(&s.quality.as_str()) {
        return Err("Unsupported quality".into());
    }
    if ![0, 1, 2, 3, 5, 10].contains(&s.concurrency) {
        return Err("Choose Automatic, 1, 2, 3, 5 or 10 concurrent jobs".into());
    }
    if !["auto", "cpu"].contains(&s.performance.as_str()) {
        return Err("Choose Automatic or CPU performance mode".into());
    }
    if !crate::output_profile::RESOLUTIONS.contains(&s.resolution) {
        return Err(
            "Clear the unsupported legacy resolution setting. Video dimensions stay unchanged."
                .into(),
        );
    }
    if s.resolution.is_some() && VIDEO_FORMATS.contains(&s.format.as_str()) {
        return Err("Video dimensions are locked to the source. Clear the legacy resolution setting; only compression quality can change.".into());
    }
    if let Some(target) = s.target_bytes {
        if target == 0 || target > 1_099_511_627_776 {
            return Err("Target size must be between 1 byte and 1 TiB.".into());
        }
        if ["wav", "flac"].contains(&s.format.as_str()) {
            return Err("Target-size compression needs a lossy format. WAV and FLAC cannot guarantee a requested size.".into());
        }
        if s.quality == "original" {
            return Err("Target-size compression requires re-encoding; Original stream copy cannot guarantee a smaller size.".into());
        }
    }
    let directory = std::fs::canonicalize(&s.output_dir)
        .map_err(|e| format!("Output folder is unavailable: {e}"))?;
    if !directory.is_dir() {
        return Err("Choose an existing output folder".into());
    }
    Ok(directory)
}
fn validate_streams(m: &Media, format: &str, quality: &str) -> Result<(), String> {
    let video_target = VIDEO_FORMATS.contains(&format);
    if video_target && m.video.is_none() {
        return Err("This file has no video stream. Choose an audio output format.".into());
    }
    if !video_target && m.audio.is_none() {
        return Err("This file has no audio stream to extract.".into());
    }
    if quality != "original" {
        return Ok(());
    }
    let video_ok = match format {
        "mp4" => &["h264", "hevc", "mpeg4", "av1"][..],
        "mov" => &["h264", "hevc", "mpeg4", "prores", "qtrle", "mjpeg", "png"][..],
        "mkv" => &[
            "h264",
            "hevc",
            "vp8",
            "vp9",
            "av1",
            "mpeg4",
            "mpeg2video",
            "mjpeg",
            "ffv1",
            "prores",
            "theora",
        ][..],
        "avi" => &[
            "h264",
            "mpeg4",
            "mpeg2video",
            "mjpeg",
            "ffv1",
            "huffyuv",
            "rawvideo",
        ][..],
        "webm" => &["vp8", "vp9", "av1"][..],
        _ => &[][..],
    };
    if video_target && !video_ok.contains(&m.video.as_deref().unwrap_or("")) {
        return Err(format!("Original video codec {} cannot be copied safely into {}. Choose Low, Medium or High to re-encode.", m.video.as_deref().unwrap_or("unknown"), format.to_uppercase()));
    }
    if let Some(audio) = m.audio.as_deref() {
        let compatible = match format {
            "mp4" => ["aac", "mp3", "alac", "ac3", "eac3"].contains(&audio),
            "mov" => {
                ["aac", "mp3", "alac", "ac3", "eac3"].contains(&audio) || audio.starts_with("pcm_")
            }
            "mkv" => {
                [
                    "aac", "mp3", "mp2", "opus", "vorbis", "flac", "alac", "ac3", "eac3",
                ]
                .contains(&audio)
                    || audio.starts_with("pcm_")
            }
            "avi" => ["mp3", "mp2", "ac3", "pcm_s16le", "pcm_s24le"].contains(&audio),
            "webm" => ["opus", "vorbis"].contains(&audio),
            "mp3" => audio == "mp3",
            "wav" => audio.starts_with("pcm_"),
            "aac" => audio == "aac",
            "m4a" => ["aac", "alac"].contains(&audio),
            "ogg" => ["vorbis", "opus", "flac"].contains(&audio),
            "flac" => audio == "flac",
            _ => false,
        };
        if !compatible {
            return Err(format!("Original audio codec {audio} cannot be copied safely into {}. Choose Low, Medium or High to re-encode.", format.to_uppercase()));
        }
    }
    Ok(())
}
fn publish_output(
    mut file: tempfile::NamedTempFile,
    directory: &Path,
    stem: &str,
    format: &str,
) -> Result<PathBuf, String> {
    for index in 1..=10000 {
        let suffix = if index == 1 {
            String::new()
        } else {
            format!("_{index}")
        };
        let p = directory.join(format!("{stem}_converted{suffix}.{format}"));
        match file.persist_noclobber(&p) {
            Ok(_) => return Ok(p),
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => file = e.file,
            Err(e) => return Err(format!("Cannot publish output: {}", e.error)),
        }
    }
    Err("Too many files share this output name".into())
}

#[derive(Default)]
struct State {
    jobs: Vec<Job>,
    running: bool,
    warnings: Vec<String>,
    active: std::collections::HashMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>,
}
#[derive(Default)]
struct Shared {
    state: std::sync::Mutex<State>,
    closed: std::sync::atomic::AtomicBool,
}
impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub struct Engine {
    runtime: PathBuf,
    shared: std::sync::Arc<Shared>,
    operations: std::sync::Mutex<()>,
    scheduler: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
    hardware_encoder: std::sync::OnceLock<bool>,
    calibration: std::sync::Arc<crate::calibration::Calibration>,
}
impl Engine {
    pub fn new(runtime: PathBuf) -> Self {
        Self::with_performance_cache(runtime, crate::calibration::configured_directory())
    }

    /// Supply a private cache directory, or disable persistent calibration with None.
    /// Calibration samples and real conversions still run through the same engine.
    pub fn with_performance_cache(runtime: PathBuf, cache: Option<PathBuf>) -> Self {
        let calibration =
            std::sync::Arc::new(crate::calibration::Calibration::new(&runtime, cache));
        Self {
            runtime,
            shared: std::sync::Arc::new(Shared::default()),
            operations: std::sync::Mutex::new(()),
            scheduler: std::sync::Mutex::new(None),
            hardware_encoder: std::sync::OnceLock::new(),
            calibration,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let state = self.shared.lock();
        Snapshot {
            jobs: state.jobs.clone(),
            running: state.running,
            warnings: state.warnings.clone(),
        }
    }

    /// Revalidate queued sources in a background worker, including when every
    /// preview is already cached. Subprocess work never holds the queue mutex.
    pub fn refresh_sources(&self) -> Result<Snapshot, String> {
        self.refresh_sources_cancellable(&std::sync::atomic::AtomicBool::new(false))
    }

    pub fn refresh_sources_cancellable(
        &self,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Snapshot, String> {
        let check = || {
            if cancelled(&self.shared.closed, Some(cancel)) {
                Err("Cancelled".to_string())
            } else {
                Ok(())
            }
        };
        let _operation = loop {
            check()?;
            match self.operations.try_lock() {
                Ok(guard) => break guard,
                Err(std::sync::TryLockError::Poisoned(error)) => break error.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(std::time::Duration::from_millis(10))
                }
            }
        };
        self.ensure_open()?;
        let jobs = {
            let state = self.shared.lock();
            if state.running {
                return Ok(Snapshot {
                    jobs: state.jobs.clone(),
                    running: true,
                    warnings: state.warnings.clone(),
                });
            }
            state
                .jobs
                .iter()
                .filter(|job| job.status == "queued")
                .cloned()
                .collect::<Vec<_>>()
        };
        for job in jobs {
            check()?;
            let path = Path::new(&job.path);
            let identity = source_identity(path);
            if identity
                .as_ref()
                .is_ok_and(|(_, revision)| revision == &job.source_revision)
            {
                continue;
            }
            let inspected: Result<(u64, String, Media), String> = (|| {
                let (size, revision) = identity.as_ref().map_err(Clone::clone)?;
                if std::fs::canonicalize(path)
                    .map_err(|e| format!("Input file is unavailable: {e}"))?
                    != path
                {
                    return Err("Input path changed. Remove the file and import it again.".into());
                }
                let media = probe(&self.runtime, path, &self.shared.closed, Some(cancel))?;
                if source_identity(path)?.1 != *revision {
                    return Err(
                        "Input changed during inspection. Retry after it finishes changing.".into(),
                    );
                }
                Ok((*size, revision.clone(), media))
            })();
            check()?;
            let mut state = self.shared.lock();
            if let Some(current) = state.jobs.iter_mut().find(|current| {
                current.id == job.id
                    && current.status == "queued"
                    && current.source_revision == job.source_revision
            }) {
                match inspected {
                    Ok((size, revision, media)) => {
                        apply_media(current, &media);
                        current.size = size;
                        current.source_revision = revision;
                        current.error = None;
                    }
                    Err(error) => {
                        if let Ok((size, revision)) = identity {
                            current.size = size;
                            current.source_revision = revision;
                        }
                        apply_media(current, &Media::default());
                        current.status = "error".into();
                        current.error = Some(format!("The source changed and could not be refreshed: {error}. Retry after the file finishes changing."));
                    }
                }
            }
        }
        Ok(self.snapshot())
    }

    /// Discover local files, keep unsupported/corrupt explicitly selected files visible,
    /// and inspect metadata without holding the queue mutex during subprocess work.
    pub fn import(&self, paths: Vec<PathBuf>) -> Result<Snapshot, String> {
        let _operation = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure_open()?;
        self.ensure_runtime()?;
        let mut scan = Scan::default();
        for path in paths {
            scan.visit(&path, 0, true, &self.shared.closed);
        }
        let mut known: std::collections::HashSet<PathBuf> = {
            let state = self.shared.lock();
            state
                .jobs
                .iter()
                .flat_map(|j| {
                    std::iter::once(PathBuf::from(&j.path))
                        .chain(j.output.as_ref().map(PathBuf::from))
                })
                .collect()
        };
        let mut duplicate_count = 0usize;
        for path in scan.files {
            if self
                .shared
                .closed
                .load(std::sync::atomic::Ordering::Acquire)
            {
                break;
            }
            let canonical = std::fs::canonicalize(&path).unwrap_or(path.clone());
            if !known.insert(canonical.clone()) {
                duplicate_count += 1;
                continue;
            }
            let metadata = std::fs::metadata(&canonical);
            let mut job = Job {
                id: uuid::Uuid::new_v4().to_string(),
                path: canonical.to_string_lossy().into_owned(),
                name: canonical
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                size: metadata.as_ref().map(|m| m.len()).unwrap_or(0),
                source_revision: metadata.as_ref().map(metadata_revision).unwrap_or_default(),
                duration: 0.0,
                video: None,
                audio: None,
                width: None,
                height: None,
                frame_rate: None,
                channels: None,
                sample_rate: None,
                sample_format: None,
                bits_per_sample: None,
                video_duration: None,
                audio_duration: None,
                video_bitrate: None,
                audio_bitrate: None,
                status: "queued".into(),
                progress: 0.0,
                output: None,
                error: None,
                output_size: None,
                output_width: None,
                output_height: None,
                output_duration: None,
                output_frame_rate: None,
                target_bytes: None,
                target_is_preset: false,
                stage: None,
                encoder: None,
                performance_note: None,
            };
            let result = if canonical.to_str().is_none() {
                Err("This filename is not valid Unicode; rename it before importing.".into())
            } else if !supported_input(&canonical) {
                Err("Unsupported input extension. Select a video or audio file.".into())
            } else if !metadata.as_ref().map(|m| m.is_file()).unwrap_or(false) {
                Err("Input file is missing or cannot be read.".into())
            } else {
                probe(&self.runtime, &canonical, &self.shared.closed, None)
            };
            match result {
                Ok(media) => apply_media(&mut job, &media),
                Err(error) => {
                    job.status = "error".into();
                    job.error = Some(error);
                }
            }
            self.shared.lock().jobs.push(job);
        }
        if duplicate_count > 0 {
            scan.warnings.push(format!(
                "Skipped {duplicate_count} duplicate or already-created output file(s)."
            ));
        }
        if scan.unsupported > 0 {
            scan.warnings.push(format!(
                "Skipped {} non-media file(s) while scanning folders.",
                scan.unsupported
            ));
        }
        if scan.limited {
            scan.warnings.push("Folder scan reached its safety limit (5,000 media files, 100,000 entries, 1,000 folders, or 10 levels). Import a smaller folder to continue.".into());
        }
        let mut state = self.shared.lock();
        state.warnings = scan.warnings;
        drop(state);
        Ok(self.snapshot())
    }

    pub fn start(&self, settings: Settings) -> Result<(), String> {
        let _operation = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure_open()?;
        self.ensure_runtime()?;
        let directory = validate_settings(&settings)?;
        // Verify write access before any job is moved into running state.
        tempfile::Builder::new()
            .prefix(".nova-write-check-")
            .tempfile_in(&directory)
            .map_err(|e| format!("Output folder is not writable: {e}"))?;
        let hardware = cfg!(target_os = "macos")
            && settings.performance == "auto"
            && settings.quality != "original"
            && ["mp4", "mov", "mkv"].contains(&settings.format.as_str())
            && *self
                .hardware_encoder
                .get_or_init(|| hardware_available(&self.runtime, &self.shared.closed));
        let resources = crate::performance::resources();
        let ids = {
            let mut state = self.shared.lock();
            if state.running {
                return Err("A batch is already running.".into());
            }
            let ids: std::collections::VecDeque<String> = state
                .jobs
                .iter()
                .filter(|j| j.status == "queued")
                .map(|j| j.id.clone())
                .collect();
            if ids.is_empty() {
                return Err("No queued files. Add files or retry a failed job.".into());
            }
            state.running = true;
            ids
        };
        let mut slot = self.scheduler.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = slot.take() {
            let _ = previous.join();
        }
        let shared = self.shared.clone();
        let runtime = self.runtime.clone();
        let calibration = self.calibration.clone();
        let maximum_pixels = shared
            .lock()
            .jobs
            .iter()
            .filter(|job| job.status == "queued")
            .map(|job| job.width.unwrap_or(0) as u64 * job.height.unwrap_or(0) as u64)
            .max()
            .unwrap_or(0);
        let schedule = crate::performance::schedule(
            resources,
            ids.len(),
            settings.concurrency,
            VIDEO_FORMATS.contains(&settings.format.as_str()),
            hardware,
            maximum_pixels,
        );
        let result = std::thread::Builder::new()
            .name("nova-batch".into())
            .spawn(move || {
                let queue = std::sync::Arc::new(std::sync::Mutex::new(ids));
                let count = schedule.workers;
                let settings = std::sync::Arc::new(settings);
                let mut workers = Vec::new();
                for index in 0..count {
                    let queue = queue.clone();
                    let worker_shared = shared.clone();
                    let runtime = runtime.clone();
                    let calibration = calibration.clone();
                    let settings = settings.clone();
                    let directory = directory.clone();
                    let result = std::thread::Builder::new()
                        .name(format!("nova-convert-{index}"))
                        .spawn(move || loop {
                            let id = queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
                            let Some(id) = id else {
                                break;
                            };
                            let token =
                                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                            let job = {
                                let mut state = worker_shared.lock();
                                let Some(job) = state.jobs.iter_mut().find(|j| j.id == id) else {
                                    continue;
                                };
                                if job.status != "queued" {
                                    continue;
                                }
                                if worker_shared
                                    .closed
                                    .load(std::sync::atomic::Ordering::Acquire)
                                {
                                    job.status = "cancelled".into();
                                    continue;
                                }
                                job.status = "running".into();
                                job.progress = 0.0;
                                job.error = None;
                                job.output = None;
                                job.output_size = None;
                                job.output_width = None;
                                job.output_height = None;
                                job.output_duration = None;
                                job.output_frame_rate = None;
                                job.target_bytes = settings.target_bytes;
                                job.target_is_preset = false;
                                job.stage = Some("Analysing".into());
                                job.encoder = None;
                                job.performance_note = None;
                                let copy = job.clone();
                                state.active.insert(id.clone(), token.clone());
                                copy
                            };
                            let outcome = convert(
                                &runtime,
                                &directory,
                                &settings,
                                &job,
                                &worker_shared,
                                &token,
                                WorkerConfiguration {
                                    threads: schedule.threads_per_worker,
                                    hardware,
                                    calibration: calibration.clone(),
                                },
                            );
                            let mut state = worker_shared.lock();
                            state.active.remove(&id);
                            if let Some(job) = state.jobs.iter_mut().find(|j| j.id == id) {
                                match outcome {
                                    Ok(path) => {
                                        job.status = "done".into();
                                        job.progress = 100.0;
                                        job.output = Some(path.to_string_lossy().into_owned());
                                        job.error = None;
                                    }
                                    Err(Failure::Cancelled) => {
                                        job.status = "cancelled".into();
                                        job.error = None;
                                        job.output = None;
                                    }
                                    Err(Failure::Error(error)) => {
                                        job.status = "error".into();
                                        job.error = Some(error);
                                        job.output = None;
                                    }
                                }
                            }
                        });
                    match result {
                        Ok(handle) => workers.push(handle),
                        Err(error) => shared
                            .lock()
                            .warnings
                            .push(format!("Could not start a conversion worker: {error}")),
                    }
                }
                for worker in workers {
                    if worker.join().is_err() {
                        shared.lock().warnings.push(
                            "A conversion worker stopped unexpectedly. Retry unfinished files."
                                .into(),
                        );
                    }
                }
                let mut state = shared.lock();
                for job in &mut state.jobs {
                    if job.status == "running" {
                        job.status = "error".into();
                        job.error =
                            Some("Conversion worker stopped unexpectedly. Retry this file.".into());
                    }
                }
                state.active.clear();
                state.running = false;
            });
        match result {
            Ok(handle) => {
                *slot = Some(handle);
                Ok(())
            }
            Err(error) => {
                self.shared.lock().running = false;
                Err(format!("Could not start batch: {error}"))
            }
        }
    }

    pub fn cancel(&self, id: Option<String>) -> Result<(), String> {
        let mut state = self.shared.lock();
        if let Some(id) = &id {
            let job = state
                .jobs
                .iter()
                .find(|j| &j.id == id)
                .ok_or("Unknown file")?;
            if !["queued", "running"].contains(&job.status.as_str()) {
                return Err("This file is not queued or running.".into());
            }
        }
        for (job_id, token) in &state.active {
            if id.as_ref().map(|id| id == job_id).unwrap_or(true) {
                token.store(true, std::sync::atomic::Ordering::Release);
            }
        }
        for job in &mut state.jobs {
            if job.status == "queued" && id.as_ref().map(|id| id == &job.id).unwrap_or(true) {
                job.status = "cancelled".into();
            }
        }
        Ok(())
    }

    pub fn retry(&self, id: String) -> Result<(), String> {
        let _operation = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        self.ensure_open()?;
        let path = {
            let state = self.shared.lock();
            let job = state
                .jobs
                .iter()
                .find(|j| j.id == id)
                .ok_or("Unknown file")?;
            if !["error", "cancelled"].contains(&job.status.as_str()) {
                return Err("Only failed or cancelled files can be retried.".into());
            }
            PathBuf::from(&job.path)
        };
        let (size, revision) = source_identity(&path)?;
        let media = probe(&self.runtime, &path, &self.shared.closed, None)?;
        if source_identity(&path)?.1 != revision {
            return Err(
                "Input changed during inspection. Retry after the file finishes changing.".into(),
            );
        }
        let mut state = self.shared.lock();
        let job = state
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or("Unknown file")?;
        apply_media(job, &media);
        job.size = size;
        job.source_revision = revision;
        job.status = "queued".into();
        job.progress = 0.0;
        job.error = None;
        job.output = None;
        job.output_size = None;
        job.output_width = None;
        job.output_height = None;
        job.output_duration = None;
        job.output_frame_rate = None;
        job.target_bytes = None;
        job.target_is_preset = false;
        job.stage = None;
        job.encoder = None;
        job.performance_note = None;
        Ok(())
    }

    pub fn remove(&self, id: String) -> Result<(), String> {
        let mut state = self.shared.lock();
        let job = state
            .jobs
            .iter()
            .find(|j| j.id == id)
            .ok_or("Unknown file")?;
        if job.status == "running" {
            return Err("Cancel this conversion before removing it.".into());
        }
        state.jobs.retain(|j| j.id != id);
        Ok(())
    }

    pub fn clear_finished(&self) -> Result<(), String> {
        self.shared.lock().jobs.retain(|j| j.status != "done");
        Ok(())
    }

    /// Stop accepting work, cancel every process, and wait for owned workers to clean up.
    pub fn shutdown(&self) {
        self.shared
            .closed
            .store(true, std::sync::atomic::Ordering::Release);
        let _ = self.cancel(None);
        let _operation = self.operations.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(handle) = self
            .scheduler
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = handle.join();
        }
    }

    fn ensure_open(&self) -> Result<(), String> {
        if self
            .shared
            .closed
            .load(std::sync::atomic::Ordering::Acquire)
        {
            Err("The converter is shutting down.".into())
        } else {
            Ok(())
        }
    }
    fn ensure_runtime(&self) -> Result<(), String> {
        for executable in ["ffmpeg", "ffprobe"] {
            if !crate::platform::executable(&self.runtime, executable).is_file() {
                return Err(format!(
                    "Bundled {executable} is missing. Reinstall the complete app."
                ));
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct Scan {
    files: Vec<PathBuf>,
    visited: std::collections::HashSet<PathBuf>,
    warnings: Vec<String>,
    directories: usize,
    entries: usize,
    unsupported: usize,
    limited: bool,
}
impl Scan {
    fn visit(
        &mut self,
        path: &Path,
        depth: usize,
        explicit: bool,
        closed: &std::sync::atomic::AtomicBool,
    ) {
        if closed.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        if self.files.len() >= 5000 || depth > 10 {
            self.limited = true;
            return;
        }
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(error) => {
                if explicit {
                    self.files.push(path.to_path_buf());
                }
                self.warn(format!("Could not read {}: {error}", path.display()));
                return;
            }
        };
        if metadata.file_type().is_symlink() {
            self.warn(format!("Skipped symbolic link: {}", path.display()));
            return;
        }
        if metadata.is_dir() {
            if self.directories >= 1000 {
                self.limited = true;
                return;
            }
            let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            if !self.visited.insert(canonical) {
                return;
            }
            self.directories += 1;
            match std::fs::read_dir(path) {
                Ok(entries) => {
                    let mut paths = Vec::new();
                    for entry in entries {
                        if closed.load(std::sync::atomic::Ordering::Acquire) {
                            return;
                        }
                        if self.entries >= 100_000 {
                            self.limited = true;
                            break;
                        }
                        self.entries += 1;
                        match entry {
                            Ok(entry) => paths.push(entry.path()),
                            Err(error) => {
                                self.warn(format!("Could not read folder entry: {error}"))
                            }
                        }
                    }
                    paths.sort();
                    for path in paths {
                        self.visit(&path, depth + 1, false, closed);
                        if self.files.len() >= 5000 {
                            break;
                        }
                    }
                }
                Err(error) => self.warn(format!("Could not scan {}: {error}", path.display())),
            }
        } else if metadata.is_file() {
            let is_temporary = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(".nova-"))
                .unwrap_or(false);
            if !is_temporary && (explicit || supported_input(path)) {
                self.files.push(path.to_path_buf());
            } else if !is_temporary {
                self.unsupported += 1;
            }
        }
    }
    fn warn(&mut self, message: String) {
        if self.warnings.len() < 100 {
            self.warnings.push(message);
        }
    }
}

fn supported_input(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    [
        "mp4", "mkv", "avi", "mov", "webm", "mp3", "wav", "aac", "ogg", "m4a", "wmv", "flv",
        "flac", "wma", "aiff", "aif", "opus", "m4v", "mpg", "mpeg", "ts", "m2ts", "ogv", "mka",
        "3gp", "caf",
    ]
    .contains(&extension.as_str())
}

fn metadata_revision(metadata: &std::fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|time| time.as_nanos())
        .unwrap_or(0);
    let mut revision = format!("{}:{modified}", metadata.len());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        revision.push_str(&format!(
            ":{}:{}:{}:{}",
            metadata.dev(),
            metadata.ino(),
            metadata.ctime(),
            metadata.ctime_nsec()
        ));
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;
        revision.push_str(&format!(
            ":{}:{}:{}",
            metadata.creation_time(),
            metadata.last_write_time(),
            metadata.file_attributes()
        ));
    }
    revision
}

fn source_identity(path: &Path) -> Result<(u64, String), String> {
    let metadata =
        std::fs::metadata(path).map_err(|e| format!("Input file is unavailable: {e}"))?;
    if !metadata.is_file() {
        return Err("Input is no longer a regular file. Remove it and import it again.".into());
    }
    Ok((metadata.len(), metadata_revision(&metadata)))
}

fn apply_media(job: &mut Job, media: &Media) {
    job.duration = media.duration;
    job.video = media.video.clone();
    job.audio = media.audio.clone();
    job.width = media.width;
    job.height = media.height;
    job.frame_rate = media.frame_rate;
    job.channels = media.channels;
    job.sample_rate = media.sample_rate;
    job.sample_format = media.sample_format.clone();
    job.bits_per_sample = media.bits_per_sample;
    job.video_duration = media.video_duration;
    job.audio_duration = media.audio_duration;
    job.video_bitrate = media.video_bitrate;
    job.audio_bitrate = media.audio_bitrate;
}

pub(crate) fn profile_streams_compatible(job: &Job, settings: &Settings) -> bool {
    validate_streams(
        &Media {
            video: job.video.clone(),
            audio: job.audio.clone(),
            ..Media::default()
        },
        &settings.format,
        &settings.quality,
    )
    .is_ok()
}

fn read_bounded(mut reader: impl std::io::Read, maximum: usize) -> (Vec<u8>, bool) {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(length) => {
                let room = maximum.saturating_sub(output.len());
                output.extend_from_slice(&buffer[..length.min(room)]);
                truncated |= length > room;
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    (output, truncated)
}

fn cancelled(
    closed: &std::sync::atomic::AtomicBool,
    token: Option<&std::sync::atomic::AtomicBool>,
) -> bool {
    closed.load(std::sync::atomic::Ordering::Acquire)
        || token
            .map(|t| t.load(std::sync::atomic::Ordering::Acquire))
            .unwrap_or(false)
}

fn probe(
    runtime: &Path,
    path: &Path,
    closed: &std::sync::atomic::AtomicBool,
    token: Option<&std::sync::atomic::AtomicBool>,
) -> Result<Media, String> {
    probe_before(
        runtime,
        path,
        closed,
        token,
        std::time::Instant::now() + std::time::Duration::from_secs(15),
    )
}

fn probe_before(
    runtime: &Path,
    path: &Path,
    closed: &std::sync::atomic::AtomicBool,
    token: Option<&std::sync::atomic::AtomicBool>,
    deadline: std::time::Instant,
) -> Result<Media, String> {
    if cancelled(closed, token) {
        return Err("Cancelled".into());
    }
    let mut child = crate::platform::command(crate::platform::executable(runtime, "ffprobe"))
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
            "-i",
        ])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Cannot start bundled media inspector: {e}"))?;
    let stdout = child.stdout.take().ok_or("Inspector output unavailable")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Inspector error output unavailable")?;
    let stdout = std::thread::spawn(move || read_bounded(stdout, 4 * 1024 * 1024));
    let stderr = std::thread::spawn(move || read_bounded(stderr, 32 * 1024));
    let status = loop {
        if cancelled(closed, token) || std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout.join();
            let _ = stderr.join();
            return Err(if cancelled(closed, token) {
                "Cancelled"
            } else {
                "Media inspection exceeded its time limit"
            }
            .into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(25)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout.join();
                let _ = stderr.join();
                return Err(format!("Media inspection failed: {error}"));
            }
        }
    };
    let (output, too_large) = stdout
        .join()
        .map_err(|_| "Media inspector output reader stopped")?;
    let (errors, _) = stderr
        .join()
        .map_err(|_| "Media inspector error reader stopped")?;
    if !status.success() {
        return Err(format!(
            "Cannot read this media file: {}",
            concise_error(&errors)
        ));
    }
    if too_large {
        return Err("Media metadata exceeds the safe inspection limit".into());
    }
    let data: serde_json::Value =
        serde_json::from_slice(&output).map_err(|e| format!("Invalid media metadata: {e}"))?;
    let streams = data["streams"].as_array().ok_or("No media streams found")?;
    let video = streams.iter().find(|s| {
        s["codec_type"] == "video" && s["disposition"]["attached_pic"].as_u64().unwrap_or(0) == 0
    });
    let audio = streams.iter().find(|s| s["codec_type"] == "audio");
    if video.is_none() && audio.is_none() {
        return Err("No playable video or audio stream was found".into());
    }
    let (display_width, display_height) = display_dimensions(video);
    let rotation = video.map(video_rotation).unwrap_or(0.0);
    let sample_aspect_ratio = video
        .and_then(|stream| stream["sample_aspect_ratio"].as_str())
        .and_then(|ratio| parse_ratio(&ratio.replace(':', "/")))
        .map(|ratio| {
            if ((rotation.abs() % 180.0) - 90.0).abs() < 0.1 {
                1.0 / ratio
            } else {
                ratio
            }
        });
    let duration = value_seconds(&data["format"]["duration"])
        .or_else(|| {
            streams
                .iter()
                .filter_map(|s| value_seconds(&s["duration"]))
                .reduce(f64::max)
        })
        .unwrap_or(0.0);
    // AVI may declare empty timing slots as frames when preserving the source
    // clock. Its header average FPS/nb_frames can describe those slots rather
    // than decoded pictures; do not display or verify them as measured cadence.
    let avi_timing = data["format"]["format_name"]
        .as_str()
        .is_some_and(|name| name.split(',').any(|part| part == "avi"));
    Ok(Media {
        duration,
        frame_rate: if avi_timing {
            None
        } else {
            video
                .and_then(|stream| stream["avg_frame_rate"].as_str())
                .and_then(parse_ratio)
        },
        video_duration: video.and_then(stream_duration),
        audio_duration: audio.and_then(stream_duration),
        video: video
            .and_then(|s| s["codec_name"].as_str())
            .map(str::to_owned),
        audio: audio
            .and_then(|s| s["codec_name"].as_str())
            .map(str::to_owned),
        width: display_width,
        height: display_height,
        sample_aspect_ratio,
        rotation,
        channels: audio
            .and_then(|s| s["channels"].as_u64())
            .and_then(|v| v.try_into().ok()),
        sample_rate: audio
            .and_then(|s| metadata_integer(&s["sample_rate"]))
            .and_then(|v| v.try_into().ok()),
        video_frames: if avi_timing {
            None
        } else {
            video.and_then(|s| metadata_integer(&s["nb_frames"]))
        },
        video_bitrate: video.and_then(|s| metadata_integer(&s["bit_rate"])),
        audio_bitrate: audio.and_then(|s| metadata_integer(&s["bit_rate"])),
        sample_format: audio
            .and_then(|s| s["sample_fmt"].as_str())
            .map(str::to_owned),
        bits_per_sample: audio.and_then(|s| {
            s["bits_per_raw_sample"]
                .as_str()
                .and_then(|v| v.parse::<u32>().ok())
                .filter(|v| *v > 0)
                .or_else(|| {
                    s["bits_per_sample"]
                        .as_u64()
                        .and_then(|v| v.try_into().ok())
                        .filter(|v| *v > 0)
                })
        }),
    })
}

// Container length can describe a longer unselected stream. Only compare known
// selected stream durations, never audio extraction against whole-container time.
fn verify_stream_duration(
    expected: Option<f64>,
    actual: Option<f64>,
    kind: &str,
) -> Result<(), String> {
    if let (Some(expected), Some(actual)) = (expected, actual) {
        if expected > 1.0 && actual < expected * 0.90 {
            return Err(format!("Output verification failed: {kind} stream is substantially shorter than the selected input stream"));
        }
    }
    Ok(())
}

fn progress_duration(media: &Media, format: &str) -> f64 {
    if !VIDEO_FORMATS.contains(&format) {
        media.audio_duration.unwrap_or(if media.video.is_none() {
            media.duration
        } else {
            0.0
        })
    } else {
        match (
            media.video_duration,
            media.audio_duration,
            media.audio.is_some(),
        ) {
            (Some(video), Some(audio), _) => video.max(audio),
            (Some(video), None, false) => video,
            _ => media.duration,
        }
    }
}

fn planning_input(media: &Media, settings: &Settings) -> crate::size_budget::Input {
    let (width, height) = (media.width.unwrap_or(0), media.height.unwrap_or(0));
    crate::size_budget::Input {
        duration: progress_duration(media, &settings.format),
        video: VIDEO_FORMATS.contains(&settings.format.as_str()) && media.video.is_some(),
        audio: media.audio.is_some(),
        width,
        height,
        frame_rate: media.frame_rate.unwrap_or(25.0),
        channels: media.channels.unwrap_or(1),
    }
}

fn display_dimensions(video: Option<&serde_json::Value>) -> (Option<u32>, Option<u32>) {
    let Some(video) = video else {
        return (None, None);
    };
    let width = video["width"].as_u64().and_then(|v| v.try_into().ok());
    let height = video["height"].as_u64().and_then(|v| v.try_into().ok());
    let rotation = video_rotation(video);
    if ((rotation.abs() % 180.0) - 90.0).abs() < 0.1 {
        (height, width)
    } else {
        (width, height)
    }
}

fn video_rotation(video: &serde_json::Value) -> f64 {
    video["side_data_list"]
        .as_array()
        .and_then(|items| items.iter().find_map(|item| item["rotation"].as_f64()))
        .or_else(|| {
            video["tags"]["rotate"]
                .as_str()
                .and_then(|value| value.parse().ok())
        })
        .filter(|rotation| rotation.is_finite())
        .unwrap_or(0.0)
}

fn stream_duration(stream: &serde_json::Value) -> Option<f64> {
    value_seconds(&stream["duration"])
        .or_else(|| {
            let ticks = stream["duration_ts"].as_f64()?;
            let base = parse_ratio(stream["time_base"].as_str()?)?;
            let seconds = ticks * base;
            (seconds.is_finite() && seconds > 0.0).then_some(seconds)
        })
        .or_else(|| {
            let text = stream["tags"]["DURATION"]
                .as_str()
                .or_else(|| stream["tags"]["duration"].as_str())?;
            let fields: Vec<&str> = text.split(':').collect();
            if fields.len() != 3 {
                return None;
            }
            let hours = fields[0].parse::<u64>().ok()?;
            let minutes = fields[1].parse::<u32>().ok()?;
            let seconds = fields[2].parse::<f64>().ok()?;
            if minutes >= 60 || !(0.0..60.0).contains(&seconds) {
                return None;
            }
            let total = hours as f64 * 3600.0 + minutes as f64 * 60.0 + seconds;
            (total > 0.0).then_some(total)
        })
}

fn parse_ratio(value: &str) -> Option<f64> {
    let (left, right) = value.split_once('/')?;
    let result = left.parse::<f64>().ok()? / right.parse::<f64>().ok()?;
    (result.is_finite() && result > 0.0).then_some(result)
}

fn value_seconds(value: &serde_json::Value) -> Option<f64> {
    let number = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|v| v.parse().ok()))?;
    (number.is_finite() && number > 0.0).then_some(number)
}

fn metadata_integer(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|value| *value > 0)
}
fn concise_error(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lines = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let message = if lines.len() <= 6 {
        lines.join("\n")
    } else {
        lines[..3]
            .iter()
            .chain(lines[lines.len() - 3..].iter())
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
    };
    if message.is_empty() {
        "The media tool exited without a diagnostic message.".into()
    } else {
        message.chars().take(3000).collect()
    }
}

#[derive(Debug)]
enum Failure {
    Cancelled,
    Error(String),
}
impl From<String> for Failure {
    fn from(value: String) -> Self {
        Self::Error(value)
    }
}

fn hardware_available(runtime: &Path, closed: &std::sync::atomic::AtomicBool) -> bool {
    // Probe an actual encode, not merely an encoder name compiled into FFmpeg.
    // The temporary process owns no user files and cannot silently select software.
    let mut child = match crate::platform::command(crate::platform::executable(runtime, "ffmpeg"))
        .args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "color=size=320x180:rate=30",
            "-frames:v",
            "2",
            "-c:v",
            "h264_videotoolbox",
            "-allow_sw",
            "0",
            "-prio_speed",
            "1",
            "-power_efficient",
            "0",
            "-b:v",
            "500000",
            "-f",
            "null",
            "-",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let started = std::time::Instant::now();
    loop {
        if closed.load(std::sync::atomic::Ordering::Acquire)
            || started.elapsed() > std::time::Duration::from_secs(8)
        {
            let _ = child.kill();
            let _ = child.wait();
            return false;
        }
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

fn use_hardware_encoder(mut arguments: Vec<String>, bitrate: Option<u64>) -> Vec<String> {
    let mut index = 0;
    while index + 1 < arguments.len() {
        if arguments[index] == "-c:v" && arguments[index + 1] == "libx264" {
            arguments[index + 1] = "h264_videotoolbox".into();
        }
        if ["-preset", "-crf"].contains(&arguments[index].as_str()) {
            arguments.drain(index..index + 2);
        } else {
            index += 1;
        }
    }
    arguments.extend(
        [
            "-allow_sw",
            "0",
            "-prio_speed",
            "1",
            "-power_efficient",
            "0",
        ]
        .into_iter()
        .map(String::from),
    );
    if let Some(rate) = bitrate {
        arguments.extend(["-b:v".into(), rate.to_string()]);
    }
    arguments
}

fn transcode_arguments(media: &Media, settings: &Settings, hardware: bool) -> Vec<String> {
    let mut args = Vec::<String>::new();
    let mut add = |values: &[&str]| args.extend(values.iter().map(|s| s.to_string()));
    let video = VIDEO_FORMATS.contains(&settings.format.as_str());
    if video {
        add(&["-map", "0:V:0", "-map", "0:a:0?"]);
    } else {
        add(&["-map", "0:a:0", "-vn"]);
    }
    add(&["-sn", "-dn", "-map_metadata", "0"]);
    if settings.quality == "original" {
        if video {
            add(&["-c:v", "copy"]);
        }
        add(&["-c:a", "copy"]);
    } else {
        let quality_index = crate::output_profile::quality_index(&settings.quality);
        let bitrate =
            crate::output_profile::nominal_audio_bitrate(&settings.format, &settings.quality)
                .to_string();
        if video {
            add(&[
                "-pix_fmt",
                if even_geometry(media) {
                    "yuv420p"
                } else {
                    "yuv444p"
                },
                "-fps_mode",
                "passthrough",
                "-enc_time_base:v",
                "-1",
            ]);
        }
        match settings.format.as_str() {
            "mp4" | "mov" | "mkv" => add(&[
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                ["30", "23", "18"][quality_index],
                "-c:a",
                "aac",
                "-b:a",
                &bitrate,
            ]),
            "avi" => add(&[
                "-c:v",
                "mpeg4",
                "-q:v",
                ["8", "5", "2"][quality_index],
                "-c:a",
                "libmp3lame",
                "-b:a",
                &bitrate,
            ]),
            "webm" => add(&[
                "-c:v",
                "libvpx-vp9",
                "-crf",
                ["38", "32", "24"][quality_index],
                "-b:v",
                "0",
                "-deadline",
                "good",
                "-cpu-used",
                "2",
                "-row-mt",
                "1",
                "-c:a",
                "libopus",
                "-b:a",
                &bitrate,
                "-ar",
                "48000",
            ]),
            "mp3" => add(&["-c:a", "libmp3lame", "-b:a", &bitrate]),
            "wav" => add(&["-c:a", wav_codec(media)]),
            "aac" | "m4a" => add(&["-c:a", "aac", "-b:a", &bitrate]),
            "ogg" => add(&["-c:a", "libvorbis", "-q:a", ["2", "5", "8"][quality_index]]),
            "flac" => add(&[
                "-c:a",
                "flac",
                "-compression_level",
                ["0", "5", "8"][quality_index],
            ]),
            _ => {}
        }
        if ["mp3", "avi", "webm"].contains(&settings.format.as_str())
            && media.channels.unwrap_or(2) > 2
        {
            add(&["-ac", "2"]);
        }
    }
    if ["mp4", "mov", "m4a"].contains(&settings.format.as_str()) {
        add(&["-movflags", "+faststart"]);
    }
    let muxer = match settings.format.as_str() {
        "mkv" => "matroska",
        "m4a" => "ipod",
        "aac" => "adts",
        other => other,
    };
    add(&["-f", muxer]);
    if hardware {
        // VideoToolbox does not implement x264 CRF. Give its hardware rate control
        // a quality-dependent picture/frame budget instead of passing ignored CRF.
        let (width, height) = (media.width.unwrap_or(640), media.height.unwrap_or(360));
        let rate = crate::output_profile::hardware_video_bitrate(
            width,
            height,
            media.frame_rate.unwrap_or(30.0),
            &settings.quality,
        );
        use_hardware_encoder(args, Some(rate))
    } else {
        args
    }
}

fn even_geometry(media: &Media) -> bool {
    matches!((media.width, media.height), (Some(width), Some(height)) if width > 0 && height > 0 && width % 2 == 0 && height % 2 == 0)
}

fn validate_video_geometry(media: &Media, settings: &Settings) -> Result<(), String> {
    if !VIDEO_FORMATS.contains(&settings.format.as_str()) {
        return Ok(());
    }
    if media.width.unwrap_or(0) == 0 || media.height.unwrap_or(0) == 0 {
        return Err("Preserving video dimensions requires readable source geometry. Re-import the file or choose a supported source.".into());
    }
    let turn = media.rotation.abs() % 90.0;
    if settings.quality != "original" && turn.min(90.0 - turn) > 0.1 {
        return Err("This video has a non-right-angle display rotation. Re-encoding could crop its picture. Choose Stream copy with a compatible format to preserve it unchanged.".into());
    }
    if settings.quality != "original" && settings.format == "avi" && !even_geometry(media) {
        return Err("AVI encoding cannot preserve this odd-sized video. Choose MP4, MOV, MKV or WebM; the source will not be resized.".into());
    }
    Ok(())
}

fn verify_video_geometry(
    source: &Media,
    output: &Media,
    settings: &Settings,
) -> Result<(), String> {
    if !VIDEO_FORMATS.contains(&settings.format.as_str()) {
        return Ok(());
    }
    verify_video_dimensions(source, output)?;
    if settings.quality == "original" && (source.rotation - output.rotation).abs() > 0.1 {
        return Err("Output verification failed: copied video orientation changed. No output was published.".into());
    }
    if let (Some(expected), Some(actual)) = (source.video_frames, output.video_frames) {
        if actual != expected {
            return Err("Output verification failed: video frame count changed. No output was published; choose another encoding format.".into());
        }
    }
    Ok(())
}

fn verify_video_dimensions(source: &Media, output: &Media) -> Result<(), String> {
    if (source.width, source.height) != (output.width, output.height) {
        return Err("Output verification failed: video dimensions changed. No output was published; choose another encoding format.".into());
    }
    let expected = source.sample_aspect_ratio.unwrap_or(1.0);
    let actual = output.sample_aspect_ratio.unwrap_or(1.0);
    if (expected - actual).abs() > expected * 0.001 {
        return Err("Output verification failed: video display aspect ratio changed. No output was published; choose another encoding format.".into());
    }
    Ok(())
}

pub(crate) fn output_sample_bits(sample_format: Option<&str>, bits_per_sample: Option<u32>) -> u32 {
    match sample_format.unwrap_or("") {
        "dbl" | "dblp" | "s64" | "s64p" => 64,
        "flt" | "fltp" => 32,
        "s32" | "s32p" if bits_per_sample.unwrap_or(32) <= 24 => 24,
        "s32" | "s32p" => 32,
        "u8" | "u8p" => 8,
        _ => 16,
    }
}

fn wav_codec(media: &Media) -> &'static str {
    match media.sample_format.as_deref().unwrap_or("") {
        "dbl" | "dblp" => "pcm_f64le",
        "flt" | "fltp" => "pcm_f32le",
        "s64" | "s64p" => "pcm_s64le",
        "s32" | "s32p"
            if output_sample_bits(media.sample_format.as_deref(), media.bits_per_sample) == 24 =>
        {
            "pcm_s24le"
        }
        "s32" | "s32p" => "pcm_s32le",
        "u8" | "u8p" => "pcm_u8",
        _ => "pcm_s16le",
    }
}

fn safe_stem(path: &Path) -> String {
    let original = path.file_stem().unwrap_or_default().to_string_lossy();
    // Bound UTF-8 bytes, not chars, to stay below filesystem component limits.
    let mut stem = String::new();
    for ch in original.chars() {
        let ch = if ch.is_control() || ['/', '\\', ':'].contains(&ch) {
            '_'
        } else {
            ch
        };
        if stem.len() + ch.len_utf8() > 150 {
            break;
        }
        stem.push(ch);
    }
    let stem = stem.trim().trim_matches('.');
    if stem.is_empty() {
        "media".into()
    } else {
        stem.into()
    }
}

fn set_stage(shared: &std::sync::Arc<Shared>, id: &str, stage: &str, reset_progress: bool) {
    if let Some(job) = shared.lock().jobs.iter_mut().find(|j| j.id == id) {
        job.stage = Some(stage.into());
        if reset_progress {
            job.progress = 0.0;
        }
    }
}

fn record_cpu_fallback(shared: &std::sync::Arc<Shared>, id: &str) {
    if let Some(job) = shared.lock().jobs.iter_mut().find(|job| job.id == id) {
        let previous = job
            .performance_note
            .take()
            .unwrap_or_else(|| "Automatic".into());
        job.performance_note = Some(format!("{previous}; final encoder: CPU fallback"));
    }
}

fn calibration_workload<'a>(
    input: &'a Path,
    media: &Media,
    settings: &'a Settings,
    threads: usize,
) -> Result<crate::calibration::Workload<'a>, Failure> {
    let (cpu_arguments, hardware_arguments) = if let Some(target) = settings.target_bytes {
        let plan = crate::size_budget::calculate(
            &planning_input(media, settings),
            &settings.format,
            target,
            1.0,
        )?;
        (
            vec![
                plan.arguments(&settings.format, true),
                plan.arguments(&settings.format, false),
            ],
            use_hardware_encoder(plan.arguments(&settings.format, false), None),
        )
    } else {
        let hardware = transcode_arguments(media, settings, true);
        let cpu = transcode_arguments(media, settings, false);
        (vec![cpu], hardware)
    };
    let duration = progress_duration(media, &settings.format);
    let sample_seconds = if duration.is_finite() && duration > 0.0 {
        duration.min(1.0)
    } else {
        1.0
    };
    let frame_rate_bucket = (media.frame_rate.unwrap_or(30.0) / 5.0).round() as u32;
    let duration_bucket = if duration < 5.0 {
        "short"
    } else if duration < 30.0 {
        "medium"
    } else {
        "long"
    };
    // The class records real processing choices as well as source properties.
    // No personal path or filename is written to the persistent cache.
    let class = format!(
        "{};{};{}x{};fps{};{};{};target{:?};threads{};audio{:?};channels{};arguments{:?}",
        media.video.as_deref().unwrap_or("unknown"),
        settings.format,
        media.width.unwrap_or(0),
        media.height.unwrap_or(0),
        frame_rate_bucket,
        duration_bucket,
        settings.quality,
        settings.target_bytes,
        threads,
        media.audio,
        media.channels.unwrap_or(1),
        cpu_arguments
    );
    Ok(crate::calibration::Workload {
        input,
        class,
        format: &settings.format,
        sample_seconds,
        threads,
        expect_audio: media.audio.is_some(),
        cpu_arguments,
        hardware_arguments,
    })
}

struct Encoding<'a> {
    runtime: &'a Path,
    input: &'a Path,
    threads: &'a str,
    shared: &'a std::sync::Arc<Shared>,
    token: &'a std::sync::Arc<std::sync::atomic::AtomicBool>,
    job_id: &'a str,
    duration: f64,
    source_revision: &'a str,
}

fn compress_to_budget(
    encoding: &Encoding<'_>,
    directory: &Path,
    settings: &Settings,
    media: &Media,
    requested_limit: u64,
    hardware: bool,
) -> Result<tempfile::NamedTempFile, Failure> {
    if hardware {
        match budget_attempts(encoding, directory, settings, media, requested_limit, true) {
            Ok(output) => return Ok(output),
            Err(Failure::Cancelled) => return Err(Failure::Cancelled),
            Err(_) => set_stage(
                encoding.shared,
                encoding.job_id,
                "Apple hardware could not fit this file — using CPU two-pass encoding",
                true,
            ),
        }
        record_cpu_fallback(encoding.shared, encoding.job_id);
    }
    budget_attempts(encoding, directory, settings, media, requested_limit, false)
}

fn budget_attempts(
    encoding: &Encoding<'_>,
    directory: &Path,
    settings: &Settings,
    media: &Media,
    requested_limit: u64,
    hardware: bool,
) -> Result<tempfile::NamedTempFile, Failure> {
    let shared = encoding.shared;
    let token = encoding.token;
    let limit = requested_limit;
    let video_target = VIDEO_FORMATS.contains(&settings.format.as_str());
    let planning_input = planning_input(media, settings);
    if video_target && (planning_input.width == 0 || planning_input.height == 0) {
        return Err(Failure::Error(
            "Target-size compression requires readable video dimensions.".into(),
        ));
    }
    let mut adjustment = 1.0;
    let mut best: Option<(tempfile::NamedTempFile, u64)> = None;
    let mut previous_plan: Option<crate::size_budget::Plan> = None;
    for attempt in 0..4 {
        if cancelled(&shared.closed, Some(token)) {
            return Err(Failure::Cancelled);
        }
        let plan = match crate::size_budget::calculate(
            &planning_input,
            &settings.format,
            limit,
            adjustment,
        ) {
            Ok(plan) => plan,
            Err(error) => {
                if best.is_some() {
                    break;
                }
                return Err(Failure::Error(error));
            }
        };
        if previous_plan.as_ref() == Some(&plan) && best.is_some() {
            break;
        }
        previous_plan = Some(plan.clone());
        let temporary = tempfile::Builder::new()
            .prefix(".nova-")
            .suffix(&format!(".part.{}", settings.format))
            .tempfile_in(directory)
            .map_err(|e| Failure::Error(format!("Cannot create size-compression output: {e}")))?;
        let encoding_result = if hardware {
            set_stage(
                shared,
                encoding.job_id,
                &format!(
                    "Apple hardware (h264_videotoolbox) — attempt {} of 4",
                    attempt + 1
                ),
                true,
            );
            run_encode(
                encoding,
                temporary.path(),
                use_hardware_encoder(plan.arguments(&settings.format, false), None),
            )
        } else if video_target {
            let pass_directory = tempfile::Builder::new()
                .prefix(".nova-pass-")
                .tempdir_in(directory)
                .map_err(|e| Failure::Error(format!("Cannot create analysis workspace: {e}")))?;
            let pass_log = pass_directory
                .path()
                .join("video")
                .to_string_lossy()
                .into_owned();
            let mut first = plan.arguments(&settings.format, true);
            first.extend([
                "-pass".into(),
                "1".into(),
                "-passlogfile".into(),
                pass_log.clone(),
            ]);
            set_stage(
                shared,
                encoding.job_id,
                &format!("Pass 1 of 2 — attempt {} of 4", attempt + 1),
                true,
            );
            let first_result = run_encode(encoding, Path::new("-"), first);
            match first_result {
                Ok(()) => {
                    let mut second = plan.arguments(&settings.format, false);
                    second.extend(["-pass".into(), "2".into(), "-passlogfile".into(), pass_log]);
                    set_stage(
                        shared,
                        encoding.job_id,
                        &format!("Pass 2 of 2 — attempt {} of 4", attempt + 1),
                        true,
                    );
                    run_encode(encoding, temporary.path(), second)
                }
                Err(error) => Err(error),
            }
            // pass_directory owns every pass-log/stat file and removes them on all exits.
        } else {
            set_stage(
                shared,
                encoding.job_id,
                &format!("Compressing audio — attempt {} of 4", attempt + 1),
                true,
            );
            run_encode(
                encoding,
                temporary.path(),
                plan.arguments(&settings.format, false),
            )
        };
        match encoding_result {
            Ok(()) => {}
            Err(Failure::Cancelled) => return Err(Failure::Cancelled),
            Err(error) => {
                if best.is_some() {
                    break;
                }
                return Err(match error {
                    Failure::Error(error) => Failure::Error(format!("The requested size could not be encoded at unchanged dimensions and frame timing. Increase the limit or choose another format. No output was published.\n{error}")),
                    cancelled => cancelled,
                });
            }
        }
        if cancelled(&shared.closed, Some(token)) {
            return Err(Failure::Cancelled);
        }
        set_stage(shared, encoding.job_id, "Verifying size", false);
        let actual = std::fs::metadata(temporary.path())
            .map_err(|e| Failure::Error(e.to_string()))?
            .len();
        if actual == 0 {
            return Err(Failure::Error(
                "Compression produced an empty output.".into(),
            ));
        }
        if actual <= limit {
            if best
                .as_ref()
                .map(|(_, size)| actual > *size)
                .unwrap_or(true)
            {
                best = Some((temporary, actual));
            }
            if actual as f64 >= limit as f64 * 0.90 {
                break;
            }
            adjustment *= (limit as f64 * 0.975 / actual as f64).clamp(1.02, 1.65);
            adjustment = adjustment.min(4.0);
        } else {
            adjustment *= (limit as f64 * 0.97 / actual as f64).clamp(0.20, 0.96);
        }
        set_stage(
            shared,
            encoding.job_id,
            "Adjusting compression to the measured size",
            false,
        );
    }
    if cancelled(&shared.closed, Some(token)) {
        return Err(Failure::Cancelled);
    }
    best.map(|(file, _)| file).ok_or_else(|| Failure::Error(format!("Could not fit the complete media below {limit} bytes after four measured attempts. Increase the target size or choose another format. No output was published.")))
}

fn run_encode(
    encoding: &Encoding<'_>,
    output: &Path,
    arguments: Vec<String>,
) -> Result<(), Failure> {
    let runtime = encoding.runtime;
    let input = encoding.input;
    let threads = encoding.threads;
    let shared = encoding.shared;
    let token = encoding.token;
    let job_id = encoding.job_id;
    let duration = encoding.duration;
    if cancelled(&shared.closed, Some(token)) {
        return Err(Failure::Cancelled);
    }
    if source_identity(input)?.1 != encoding.source_revision {
        return Err(Failure::Error("Input changed during conversion. No output was published. Retry to inspect the current file.".into()));
    }
    let encoder = arguments
        .windows(2)
        .find(|pair| pair[0] == "-c:v")
        .or_else(|| arguments.windows(2).find(|pair| pair[0] == "-c:a"))
        .map(|pair| pair[1].clone());
    if let Some(job) = shared.lock().jobs.iter_mut().find(|j| j.id == job_id) {
        job.encoder = encoder;
    }
    let mut command = crate::platform::command(crate::platform::executable(runtime, "ffmpeg"));
    command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "file,pipe",
            "-threads",
            threads,
            "-i",
        ])
        .arg(input)
        .args([
            "-threads",
            threads,
            "-filter_threads",
            threads,
            "-filter_complex_threads",
            threads,
        ])
        .args(arguments)
        .args(["-progress", "pipe:1", "-nostats", "-y"])
        .arg(output)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| Failure::Error(format!("Cannot start converter: {e}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Failure::Error("Conversion progress pipe unavailable".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Failure::Error("Conversion error pipe unavailable".into()))?;
    let progress_shared = shared.clone();
    let id = job_id.to_owned();
    let progress_reader = std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
        {
            if let Some(value) = line.strip_prefix("out_time_us=") {
                if let Ok(microseconds) = value.parse::<f64>() {
                    if duration > 0.0 && microseconds.is_finite() {
                        let progress =
                            (microseconds / 1_000_000.0 / duration * 100.0).clamp(0.0, 99.5);
                        if let Some(job) =
                            progress_shared.lock().jobs.iter_mut().find(|j| j.id == id)
                        {
                            job.progress = job.progress.max(progress);
                        }
                    }
                }
            }
        }
    });
    let error_reader = std::thread::spawn(move || read_bounded(stderr, 64 * 1024));
    let result = loop {
        if cancelled(&shared.closed, Some(token)) {
            let _ = child.kill();
            let _ = child.wait();
            break Err(Failure::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(30)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(Failure::Error(format!(
                    "Conversion process failed: {error}"
                )));
            }
        }
    };
    let _ = progress_reader.join();
    let (errors, _) = error_reader.join().unwrap_or_default();
    let status = result?;
    if cancelled(&shared.closed, Some(token)) {
        return Err(Failure::Cancelled);
    }
    if !status.success() {
        return Err(Failure::Error(format!(
            "Conversion failed: {}",
            concise_error(&errors)
        )));
    }
    Ok(())
}

#[derive(Clone)]
struct WorkerConfiguration {
    threads: usize,
    hardware: bool,
    calibration: std::sync::Arc<crate::calibration::Calibration>,
}

fn convert(
    runtime: &Path,
    directory: &Path,
    settings: &Settings,
    job: &Job,
    shared: &std::sync::Arc<Shared>,
    token: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: WorkerConfiguration,
) -> Result<PathBuf, Failure> {
    let check = || {
        if cancelled(&shared.closed, Some(token)) {
            Err(Failure::Cancelled)
        } else {
            Ok(())
        }
    };
    check()?;
    let input = PathBuf::from(&job.path);
    let canonical = std::fs::canonicalize(&input)
        .map_err(|e| Failure::Error(format!("Input file is unavailable: {e}")))?;
    if canonical != input || !canonical.is_file() {
        return Err(Failure::Error(
            "Input path changed after selection. Remove it and import the file again.".into(),
        ));
    }
    if std::fs::canonicalize(directory).map_err(|e| Failure::Error(e.to_string()))? != directory {
        return Err(Failure::Error(
            "Output folder changed after selection.".into(),
        ));
    }
    let (source_size, source_revision) = source_identity(&input)?;
    let media = match probe(runtime, &input, &shared.closed, Some(token)) {
        Ok(media) => media,
        Err(error) => {
            check()?;
            return Err(error.into());
        }
    };
    check()?;
    if source_identity(&input)?.1 != source_revision {
        return Err(Failure::Error(
            "Input changed during inspection. Retry after the file finishes changing.".into(),
        ));
    }
    let (profile, target_is_preset) = compression_profile(settings, source_size)?;
    let settings = &profile;
    validate_streams(&media, &settings.format, &settings.quality)?;
    validate_video_geometry(&media, settings)?;
    {
        let mut state = shared.lock();
        if let Some(current) = state.jobs.iter_mut().find(|j| j.id == job.id) {
            apply_media(current, &media);
            current.size = source_size;
            current.source_revision = source_revision.clone();
            current.target_bytes = settings.target_bytes;
            current.target_is_preset = target_is_preset;
        }
    }
    let threads = worker.threads.max(1).to_string();
    let mut hardware = worker.hardware && even_geometry(&media);
    if hardware {
        let workload = calibration_workload(&input, &media, settings, worker.threads)?;
        set_stage(shared, &job.id, "Calibrating automatic performance", true);
        let selection = worker
            .calibration
            .select(runtime, &workload, directory, &shared.closed, token)
            .map_err(|()| Failure::Cancelled)?;
        hardware = selection.hardware;
        if let Some(current) = shared
            .lock()
            .jobs
            .iter_mut()
            .find(|current| current.id == job.id)
        {
            current.performance_note = Some(selection.description());
        }
    } else if let Some(current) = shared
        .lock()
        .jobs
        .iter_mut()
        .find(|current| current.id == job.id)
    {
        current.performance_note = Some(if settings.quality == "original" { "Original stream copy" }
            else if settings.performance == "cpu" { "CPU mode" }
            else { "Automatic: CPU; Apple H.264 hardware is unavailable or incompatible with this output" }.into());
    }
    let encoding = Encoding {
        runtime,
        input: &input,
        threads: &threads,
        shared,
        token,
        job_id: &job.id,
        duration: progress_duration(&media, &settings.format),
        source_revision: &source_revision,
    };
    let temporary = if let Some(target) = settings.target_bytes {
        match compress_to_budget(&encoding, directory, settings, &media, target, hardware) {
            Err(Failure::Error(error)) if target_is_preset => return Err(Failure::Error(format!("The selected source-relative preset cannot fit this complete video at unchanged dimensions and frame timing. Choose a higher preset, a larger explicit size limit, or Stream copy.\n{error}"))),
            outcome => outcome?,
        }
    } else {
        let temporary = tempfile::Builder::new()
            .prefix(".nova-")
            .suffix(&format!(".part.{}", settings.format))
            .tempfile_in(directory)
            .map_err(|e| Failure::Error(format!("Cannot create temporary output: {e}")))?;
        set_stage(
            shared,
            &job.id,
            if hardware {
                "Converting with Apple hardware (h264_videotoolbox)"
            } else {
                "Converting with CPU"
            },
            false,
        );
        let result = run_encode(
            &encoding,
            temporary.path(),
            transcode_arguments(&media, settings, hardware),
        );
        match result {
            Ok(()) => {}
            Err(Failure::Cancelled) => return Err(Failure::Cancelled),
            Err(_) if hardware => {
                record_cpu_fallback(shared, &job.id);
                set_stage(
                    shared,
                    &job.id,
                    "Apple hardware unavailable for this file — using CPU",
                    true,
                );
                run_encode(
                    &encoding,
                    temporary.path(),
                    transcode_arguments(&media, settings, false),
                )?;
            }
            Err(error) => return Err(error),
        }
        temporary
    };
    set_stage(shared, &job.id, "Verifying output", false);
    let output_media = match probe(runtime, temporary.path(), &shared.closed, Some(token)) {
        Ok(media) => media,
        Err(error) => {
            check()?;
            return Err(Failure::Error(format!(
                "Output verification failed: {error}"
            )));
        }
    };
    validate_streams(&output_media, &settings.format, "medium")?;
    if media.audio.is_some() && output_media.audio.is_none() {
        return Err(Failure::Error(
            "Output verification failed: audio stream is missing".into(),
        ));
    }
    if std::fs::metadata(temporary.path())
        .map(|m| m.len())
        .unwrap_or(0)
        == 0
    {
        return Err(Failure::Error(
            "Output verification failed: empty file".into(),
        ));
    }
    verify_video_geometry(&media, &output_media, settings)?;
    verify_stream_duration(media.audio_duration, output_media.audio_duration, "audio")?;
    if VIDEO_FORMATS.contains(&settings.format.as_str()) {
        verify_stream_duration(media.video_duration, output_media.video_duration, "video")?;
    }
    check()?;
    let final_size = std::fs::metadata(temporary.path())
        .map_err(|e| Failure::Error(e.to_string()))?
        .len();
    if settings
        .target_bytes
        .is_some_and(|limit| final_size > limit)
    {
        return Err(Failure::Error("Size verification failed: the completed output exceeds the requested limit. No output was published.".into()));
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|e| Failure::Error(format!("Cannot flush output: {e}")))?;
    // Coordinate final publication with cancel(): either cancellation wins and no file
    // is published, or the completed result becomes visible atomically.
    let mut state = shared.lock();
    check()?;
    if source_identity(&input)?.1 != source_revision {
        return Err(Failure::Error("Input changed during conversion. No output was published. Retry to inspect the current file.".into()));
    }
    let output = publish_output(temporary, directory, &safe_stem(&input), &settings.format)?;
    if let Some(current) = state.jobs.iter_mut().find(|j| j.id == job.id) {
        current.status = "done".into();
        current.progress = 100.0;
        current.output = Some(output.to_string_lossy().into_owned());
        current.output_size = Some(final_size);
        current.output_width = output_media.width;
        current.output_height = output_media.height;
        current.output_duration = Some(progress_duration(&output_media, &settings.format))
            .filter(|duration| duration.is_finite() && *duration > 0.0);
        current.output_frame_rate = output_media.frame_rate;
        current.target_bytes = settings.target_bytes;
        current.target_is_preset = target_is_preset;
        current.stage = Some(
            if settings
                .target_bytes
                .is_some_and(|limit| final_size as f64 / (limit as f64) < 0.85)
            {
                "Complete — below size limit; exact size depends on the content"
            } else {
                "Complete"
            }
            .into(),
        );
        current.error = None;
    }
    state.active.remove(&job.id);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn settings(directory: &Path) -> Settings {
        Settings {
            format: "mp4".into(),
            quality: "medium".into(),
            output_dir: directory.to_string_lossy().into(),
            concurrency: 3,
            performance: "auto".into(),
            target_bytes: None,
            resolution: None,
        }
    }
    // Catches accepting untrusted format strings, invalid quality and runaway worker counts.
    #[test]
    fn invalid_batch_settings_are_rejected_before_spawning() {
        let tmp = tempfile::tempdir().unwrap();
        for (format, quality, concurrency) in [
            ("../../mp4", "medium", 3),
            ("mp4", "ultra", 3),
            ("mp4", "high", 999),
        ] {
            let s = Settings {
                format: format.into(),
                quality: quality.into(),
                concurrency,
                ..settings(tmp.path())
            };
            assert!(validate_settings(&s).is_err());
        }
        let s = Settings {
            output_dir: tmp.path().join("missing").to_string_lossy().into(),
            ..settings(tmp.path())
        };
        assert!(validate_settings(&s).is_err());
    }
    #[test]
    fn performance_settings_remain_backward_compatible_and_validate_modes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut value = serde_json::to_value(settings(tmp.path())).unwrap();
        value.as_object_mut().unwrap().remove("performance");
        value["concurrency"] = 0.into();
        let automatic: Settings = serde_json::from_value(value).unwrap();
        assert_eq!(automatic.performance, "auto");
        assert!(validate_settings(&automatic).is_ok());
        assert!(validate_settings(&Settings {
            performance: "cpu".into(),
            ..automatic.clone()
        })
        .is_ok());
        assert!(validate_settings(&Settings {
            performance: "invalid".into(),
            ..automatic
        })
        .is_err());
    }
    #[test]
    fn video_resize_settings_are_rejected_for_every_quality() {
        let tmp = tempfile::tempdir().unwrap();
        for quality in ["low", "medium", "high", "original"] {
            let config = Settings {
                resolution: Some(720),
                quality: quality.into(),
                ..settings(tmp.path())
            };
            assert!(validate_settings(&config).is_err());
        }
    }
    #[test]
    fn production_profiles_preserve_source_geometry_and_timing() {
        let tmp = tempfile::tempdir().unwrap();
        let config = settings(tmp.path());
        let media = Media {
            width: Some(3840),
            height: Some(2160),
            frame_rate: Some(60.0),
            video: Some("h264".into()),
            audio: Some("aac".into()),
            ..Media::default()
        };
        for hardware in [false, true] {
            let arguments = transcode_arguments(&media, &config, hardware);
            assert!(!arguments
                .iter()
                .any(|argument| ["-vf", "-r"].contains(&argument.as_str())));
            assert!(arguments
                .windows(2)
                .any(|pair| pair == ["-fps_mode", "passthrough"]));
        }
        let input = planning_input(&media, &config);
        assert_eq!(
            (input.width, input.height, input.frame_rate),
            (3840, 2160, 60.0)
        );
        let odd = Media {
            width: Some(641),
            height: Some(361),
            ..media
        };
        let arguments = transcode_arguments(&odd, &config, false);
        assert!(arguments
            .windows(2)
            .any(|pair| pair == ["-pix_fmt", "yuv444p"]));
    }
    #[test]
    fn explicit_limits_remain_the_requested_maximum_even_when_larger_than_source() {
        let directory = tempfile::tempdir().unwrap();
        let settings = Settings {
            target_bytes: Some(1_000_000),
            ..settings(directory.path())
        };
        let (applied, preset) = compression_profile(&settings, 100_000).unwrap();
        assert_eq!(applied.target_bytes, Some(1_000_000));
        assert!(!preset);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_attribute_changes_invalidate_source_identity_even_when_bytes_and_mtime_match() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("identity.dat");
        std::fs::write(&path, b"unchanged bytes").unwrap();
        let before = std::fs::metadata(&path).unwrap();
        let expected = metadata_revision(&before);
        let original_permissions = before.permissions();
        let mut changed_permissions = original_permissions.clone();
        changed_permissions.set_readonly(true);
        std::fs::set_permissions(&path, changed_permissions).unwrap();
        let after = std::fs::metadata(&path).unwrap();
        let actual = metadata_revision(&after);
        std::fs::set_permissions(&path, original_permissions).unwrap();
        assert_eq!(before.len(), after.len());
        assert_eq!(before.modified().unwrap(), after.modified().unwrap());
        assert_ne!(expected, actual);
    }

    // Catches silent loss of video/audio and impossible remux promises.
    #[test]
    fn stream_validation_rejects_missing_streams_and_incompatible_copy() {
        let audio = Media {
            audio: Some("aac".into()),
            ..Media::default()
        };
        assert!(validate_streams(&audio, "mp4", "medium").is_err());
        assert!(validate_streams(&audio, "mp3", "original").is_err());
        assert!(validate_streams(&audio, "m4a", "original").is_ok());
        let video = Media {
            video: Some("h264".into()),
            audio: Some("aac".into()),
            ..Media::default()
        };
        assert!(validate_streams(&video, "webm", "original").is_err());
        assert!(validate_streams(&video, "mp4", "original").is_ok());
        let silent = Media {
            video: Some("h264".into()),
            ..Media::default()
        };
        assert!(validate_streams(&silent, "mp3", "medium").is_err());
        assert!(validate_streams(&silent, "mp4", "medium").is_ok());
    }
    // Catches the old -y collision behavior while preserving Unicode user filenames.
    #[test]
    fn publication_never_overwrites_existing_destinations() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("آهنگ ; $test_converted.wav");
        std::fs::write(&existing, b"existing personal data").unwrap();
        let mut temporary = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        temporary.write_all(b"new media").unwrap();
        let result = publish_output(temporary, dir.path(), "آهنگ ; $test", "wav").unwrap();
        assert_ne!(existing, result);
        assert_eq!(std::fs::read(existing).unwrap(), b"existing personal data");
        assert_eq!(std::fs::read(result).unwrap(), b"new media");
    }
}
