//! Native Rust desktop interface. Media work never runs on the drawing thread.
use eframe::egui::{self, Color32, FontId, RichText, Stroke, Vec2};
use nova_flux::engine::{Engine, Job, MeasuredEstimate, Settings, Snapshot};
use nova_flux::output_profile::{self, Estimate};
use nova_flux::platform;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

const CORAL: Color32 = Color32::from_rgb(236, 121, 95);
const NAVY: Color32 = Color32::from_rgb(25, 41, 54);
const GREEN: Color32 = Color32::from_rgb(83, 126, 101);
const VIDEO: &[&str] = &["mp4", "mkv", "avi", "mov", "webm"];
const AUDIO: &[&str] = &["mp3", "wav", "aac", "ogg", "m4a", "flac"];
const QUALITY: &[(&str, &str)] = &[
    ("low", "Compact"),
    ("medium", "Balanced"),
    ("high", "High quality"),
    ("original", "Stream copy"),
];

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Preferences {
    settings: Settings,
    dark: bool,
    budget_enabled: bool,
    budget_draft: String,
    ordinary_quality: String,
    settings_open: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            settings: Settings {
                format: "mp4".into(),
                quality: "medium".into(),
                output_dir: String::new(),
                concurrency: 0,
                performance: "auto".into(),
                target_bytes: None,
                resolution: None,
            },
            dark: false,
            budget_enabled: false,
            budget_draft: "1".into(),
            ordinary_quality: "medium".into(),
            settings_open: true,
        }
    }
}

impl Preferences {
    fn sanitize(&mut self) {
        if !VIDEO.contains(&self.settings.format.as_str())
            && !AUDIO.contains(&self.settings.format.as_str())
        {
            self.settings.format = "mp4".into();
        }
        if !QUALITY.iter().any(|(key, _)| *key == self.settings.quality) {
            self.settings.quality = "medium".into();
        }
        if !QUALITY.iter().any(|(key, _)| *key == self.ordinary_quality) {
            self.ordinary_quality = "medium".into();
        }
        if ![0, 1, 2, 3, 5, 10].contains(&self.settings.concurrency) {
            self.settings.concurrency = 0;
        }
        if !["auto", "cpu"].contains(&self.settings.performance.as_str()) {
            self.settings.performance = "auto".into();
        }
        // Legacy resize preferences cannot override the source-geometry contract.
        self.settings.resolution = None;
        if self.budget_draft.len() > 64 {
            self.budget_draft = "1".into();
        }
        if self.settings.output_dir.len() > 16_384 {
            self.settings.output_dir.clear();
        }
        self.apply_budget();
    }

    fn target(&self) -> Option<u64> {
        // A native field receives the active keyboard's digits even though the
        // interface is English. Accept Persian and Arabic numeric keyboards.
        let normalized: String = self
            .budget_draft
            .trim()
            .chars()
            .map(|character| match character {
                '\u{06f0}'..='\u{06f9}' => (b'0' + (character as u32 - 0x06f0) as u8) as char,
                '\u{0660}'..='\u{0669}' => (b'0' + (character as u32 - 0x0660) as u8) as char,
                '\u{066b}' => '.',
                _ => character,
            })
            .collect();
        let value = normalized.parse::<f64>().ok()?;
        if !value.is_finite() || !(0.001..=1_099_511.627_776).contains(&value) {
            return None;
        }
        Some((value * 1_000_000.0).round() as u64)
    }

    fn apply_budget(&mut self) {
        self.settings.resolution = None;
        self.settings.target_bytes = if self.budget_enabled {
            self.target()
        } else {
            None
        };
        if self.budget_enabled {
            if ["wav", "flac"].contains(&self.settings.format.as_str()) {
                self.settings.format = "mp3".into();
            }
            if self.settings.quality == "original" {
                self.settings.quality = "medium".into();
            }
        }
        if self.settings.quality == "original" && VIDEO.contains(&self.settings.format.as_str()) {
            self.settings.resolution = None;
        }
    }
}

fn preference_path() -> Option<PathBuf> {
    platform::data_dir()
        .ok()
        .map(|dir| dir.join("preferences.json"))
}

fn read_preferences() -> Preferences {
    let mut preferences = preference_path()
        .and_then(|path| std::fs::read(path).ok())
        .filter(|bytes| bytes.len() <= 65_536)
        .and_then(|bytes| serde_json::from_slice::<Preferences>(&bytes).ok())
        .unwrap_or_default();
    preferences.sanitize();
    preferences
}

fn write_preferences(preferences: &Preferences) -> Result<(), String> {
    let path = preference_path().ok_or("The preferences folder is unavailable.")?;
    let directory = path.parent().ok_or("The preferences path is invalid.")?;
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let data = serde_json::to_vec_pretty(preferences).map_err(|error| error.to_string())?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
    temporary
        .write_all(&data)
        .map_err(|error| error.to_string())?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    temporary
        .persist(path)
        .map_err(|error| error.error.to_string())?;
    Ok(())
}

/// Requests receive their revision on the drawing thread before dispatch.
/// Serialization alone is insufficient: a delayed old worker must never replace
/// the newer preferences written while the application is closing.
#[derive(Default)]
struct PreferenceWriter {
    last_revision: Mutex<u64>,
}

impl PreferenceWriter {
    fn save(&self, revision: u64, preferences: &Preferences) -> Result<(), String> {
        let mut last_revision = self
            .last_revision
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if revision < *last_revision {
            return Ok(());
        }
        *last_revision = revision;
        write_preferences(preferences)
    }
}

enum WorkerMessage {
    Finished(Result<Snapshot, String>),
    Output(Option<PathBuf>),
    PreferencesSaved(Result<(), String>),
    Preview(u64, String, Result<MeasuredEstimate, String>),
    Sources(Result<Snapshot, String>),
    Closed,
}

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Active,
    Done,
    Attention,
}

struct SmokeTest {
    started: Instant,
    duration: Duration,
    frames: u64,
    requested: bool,
    completed: bool,
    destination: PathBuf,
}

impl SmokeTest {
    fn new(cc: &eframe::CreationContext<'_>) -> Option<Self> {
        let duration = platform::smoke_test_duration()?;
        let destination = std::env::var_os("NOVA_SMOKE_SCREENSHOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("nova-flux-smoke.png"));
        let adapter = cc
            .wgpu_render_state
            .as_ref()
            .map(|state| state.adapter.get_info());
        println!(
            "{}",
            serde_json::json!({"event":"smoke_started","app_version":env!("CARGO_PKG_VERSION"),"platform":std::env::consts::OS,"adapter":format!("{adapter:?}")})
        );
        Some(Self {
            started: Instant::now(),
            duration,
            frames: 0,
            requested: false,
            completed: false,
            destination,
        })
    }

    fn update(&mut self, ctx: &egui::Context) {
        self.frames += 1;
        ctx.request_repaint_after(Duration::from_millis(30));
        let captured = ctx.input(|input| {
            input.events.iter().find_map(|event| {
                if let egui::Event::Screenshot { image, .. } = event {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if !self.completed {
            if let Some(captured) = captured {
                let pixels: Vec<u8> = captured
                    .pixels
                    .iter()
                    .flat_map(|color| color.to_array())
                    .collect();
                if let Some(parent) = self.destination.parent() {
                    std::fs::create_dir_all(parent).expect("smoke screenshot directory");
                }
                image::save_buffer(
                    &self.destination,
                    &pixels,
                    captured.width() as u32,
                    captured.height() as u32,
                    image::ColorType::Rgba8,
                )
                .expect("smoke PNG screenshot");
                let unique_colors = captured
                    .pixels
                    .iter()
                    .map(|pixel| pixel.to_array())
                    .collect::<HashSet<_>>()
                    .len();
                assert!(
                    unique_colors > 16,
                    "native smoke screenshot has no rendered UI detail"
                );
                self.completed = true;
                println!(
                    "{}",
                    serde_json::json!({"event":"smoke_complete","app_version":env!("CARGO_PKG_VERSION"),"platform":std::env::consts::OS,"frames":self.frames,"screenshot":self.destination,"width":captured.width(),"height":captured.height()})
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if !self.requested && self.frames >= 3 && self.started.elapsed() >= self.duration {
            self.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        assert!(
            self.completed || self.started.elapsed() < self.duration + Duration::from_secs(15),
            "native renderer did not return the requested smoke screenshot"
        );
    }
}

struct Desktop {
    engine: Arc<Engine>,
    snapshot: Snapshot,
    preferences: Preferences,
    tx: mpsc::Sender<WorkerMessage>,
    rx: mpsc::Receiver<WorkerMessage>,
    operation: Option<String>,
    error: Option<String>,
    notice: Option<String>,
    filter: Filter,
    next_snapshot: Instant,
    save_at: Option<Instant>,
    saving: bool,
    preference_writer: Arc<PreferenceWriter>,
    preference_revision: u64,
    closing: bool,
    close_ready: bool,
    logo: Option<egui::TextureHandle>,
    previews: HashMap<String, Result<MeasuredEstimate, String>>,
    preview_signature: String,
    preview_cancel: Arc<AtomicBool>,
    preview_worker: Option<std::thread::JoinHandle<()>>,
    preview_generation: u64,
    source_worker: Option<std::thread::JoinHandle<()>>,
    source_cancel: Arc<AtomicBool>,
    sources_at: Instant,
    smoke: Option<SmokeTest>,
}

impl Desktop {
    fn new(cc: &eframe::CreationContext<'_>, engine: Arc<Engine>) -> Self {
        install_fonts(&cc.egui_ctx);
        let preferences = read_preferences();
        apply_style(&cc.egui_ctx, preferences.dark);
        let logo = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
            .ok()
            .map(|icon| {
                cc.egui_ctx.load_texture(
                    "nova-mark",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [icon.width as usize, icon.height as usize],
                        &icon.rgba,
                    ),
                    egui::TextureOptions::LINEAR,
                )
            });
        let (tx, rx) = mpsc::channel();
        let snapshot = engine.snapshot();
        Self {
            engine,
            snapshot,
            preferences,
            tx,
            rx,
            operation: None,
            error: None,
            notice: None,
            filter: Filter::All,
            next_snapshot: Instant::now(),
            save_at: None,
            saving: false,
            preference_writer: Arc::new(PreferenceWriter::default()),
            preference_revision: 0,
            closing: false,
            close_ready: false,
            logo,
            previews: HashMap::new(),
            preview_signature: String::new(),
            preview_cancel: Arc::new(AtomicBool::new(false)),
            preview_worker: None,
            preview_generation: 0,
            source_worker: None,
            source_cancel: Arc::new(AtomicBool::new(false)),
            sources_at: Instant::now(),
            smoke: SmokeTest::new(cc),
        }
    }

    fn busy(&self) -> bool {
        self.operation.is_some() || self.closing
    }

    fn processing(&self) -> bool {
        self.snapshot.running || self.snapshot.jobs.iter().any(|job| job.status == "running")
    }

    fn take_preview_worker(&mut self) -> Option<std::thread::JoinHandle<()>> {
        self.invalidate_previews();
        self.preview_worker.take()
    }

    fn invalidate_previews(&mut self) {
        self.preview_cancel.store(true, Ordering::Release);
        self.preview_signature.clear();
        self.preview_generation = self.preview_generation.wrapping_add(1);
    }

    fn take_source_worker(&mut self) -> Option<std::thread::JoinHandle<()>> {
        self.source_cancel.store(true, Ordering::Release);
        self.source_worker.take()
    }

    fn refresh_source_metadata(&mut self, ctx: &egui::Context) {
        if self.busy()
            || self.processing()
            || Instant::now() < self.sources_at
            || !self.snapshot.jobs.iter().any(|job| job.status == "queued")
        {
            return;
        }
        if let Some(worker) = &self.source_worker {
            if !worker.is_finished() {
                return;
            }
        }
        if let Some(worker) = self.source_worker.take() {
            let _ = worker.join();
        }
        self.sources_at = Instant::now() + Duration::from_secs(2);
        let engine = self.engine.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        self.source_cancel = cancel.clone();
        self.source_worker = Some(std::thread::spawn(move || {
            let result = engine.refresh_sources_cancellable(&cancel);
            if !cancel.load(Ordering::Acquire) {
                let _ = tx.send(WorkerMessage::Sources(result));
                ctx.request_repaint();
            }
        }));
    }

    fn refresh_previews(&mut self, ctx: &egui::Context) {
        if self.busy() || self.processing() || self.preferences.budget_enabled {
            self.invalidate_previews();
            return;
        }
        let jobs: Vec<Job> = self
            .snapshot
            .jobs
            .iter()
            .filter(|job| job.status == "queued")
            .cloned()
            .collect();
        let mut settings = self.preferences.settings.clone();
        settings.target_bytes = None;
        // Only encoding-relevant fields invalidate local measurements.
        settings.output_dir.clear();
        let signature = format!(
            "{:?}|{}",
            settings,
            jobs.iter()
                .map(|job| format!("{}:{}:{}", job.id, job.source_revision, job.size))
                .collect::<Vec<_>>()
                .join("|")
        );
        if signature == self.preview_signature {
            return;
        }
        if self.preview_worker.is_some() {
            // Clear A before waiting for its cancellation, including A→B→A.
            self.invalidate_previews();
            let worker = self.preview_worker.as_ref().unwrap();
            if !worker.is_finished() {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
        }
        if let Some(worker) = self.preview_worker.take() {
            let _ = worker.join();
        }
        self.preview_signature = signature;
        let mut profiles = Vec::new();
        for (quality, _) in QUALITY {
            let mut candidate = settings.clone();
            candidate.quality = (*quality).into();
            if *quality == "original" {
                candidate.resolution = None;
            }
            profiles.push(candidate);
        }
        // The selected output is measured first so useful evidence appears early.
        profiles.insert(0, settings);
        let mut seen = HashSet::new();
        let context = preview_context(jobs.iter());
        let mut requests = Vec::new();
        for profile in profiles {
            for job in &jobs {
                if output_profile::preset_limit(job, &profile).is_some() {
                    continue;
                }
                let key = preview_key(job, &profile, context);
                if seen.insert(key.clone()) && !self.previews.contains_key(&key) {
                    requests.push((key, job.clone(), profile.clone()));
                }
            }
        }
        if requests.is_empty() {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.preview_cancel = cancel.clone();
        let generation = self.preview_generation;
        let engine = self.engine.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.preview_worker = Some(std::thread::spawn(move || {
            for (key, job, profile) in requests {
                if cancel.load(Ordering::Acquire) {
                    break;
                }
                let result = engine.measure_estimate(&job, &profile, &cancel);
                if cancel.load(Ordering::Acquire) {
                    break;
                }
                if tx
                    .send(WorkerMessage::Preview(generation, key, result))
                    .is_err()
                {
                    break;
                }
                ctx.request_repaint();
            }
        }));
    }

    fn changed(&mut self, ctx: &egui::Context) {
        self.preferences.apply_budget();
        self.save_at = Some(Instant::now() + Duration::from_millis(400));
        ctx.request_repaint_after(Duration::from_millis(400));
    }

    fn work(
        &mut self,
        ctx: &egui::Context,
        label: &str,
        action: impl FnOnce(Arc<Engine>) -> Result<Snapshot, String> + Send + 'static,
    ) {
        if self.busy() {
            return;
        }
        self.operation = Some(label.into());
        self.error = None;
        let preview = self.take_preview_worker();
        let sources = self.take_source_worker();
        let engine = self.engine.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            if let Some(preview) = preview {
                let _ = preview.join();
            }
            if let Some(sources) = sources {
                let _ = sources.join();
            }
            let _ = tx.send(WorkerMessage::Finished(action(engine)));
            ctx.request_repaint();
        });
    }

    fn pick_inputs(&mut self, ctx: &egui::Context, folders: bool) {
        if self.busy() || self.processing() {
            return;
        }
        self.work(
            ctx,
            if folders {
                "Scanning folder…"
            } else {
                "Reading files…"
            },
            move |engine| {
                let picker = rfd::FileDialog::new().set_title(if folders {
                    "Add media folders"
                } else {
                    "Add media files"
                });
                let paths = if folders {
                    picker.pick_folders()
                } else {
                    picker
                        .add_filter(
                            "Audio and video",
                            &[
                                "mp4", "mkv", "avi", "mov", "webm", "wmv", "flv", "mp3", "wav",
                                "aac", "ogg", "m4a", "flac", "wma", "aiff", "aif", "opus", "m4v",
                                "mpg", "mpeg", "ts", "m2ts",
                            ],
                        )
                        .pick_files()
                };
                match paths {
                    Some(paths) => engine.import(paths),
                    None => Ok(engine.snapshot()),
                }
            },
        );
    }

    fn pick_output(&mut self, ctx: &egui::Context) {
        if self.busy() || self.processing() {
            return;
        }
        self.operation = Some("Choosing output folder…".into());
        let preview = self.take_preview_worker();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let current = self.preferences.settings.output_dir.clone();
        std::thread::spawn(move || {
            if let Some(preview) = preview {
                let _ = preview.join();
            }
            let mut picker = rfd::FileDialog::new().set_title("Choose output folder");
            if !current.is_empty() {
                picker = picker.set_directory(current);
            }
            let _ = tx.send(WorkerMessage::Output(picker.pick_folder()));
            ctx.request_repaint();
        });
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.start_problem().is_some() || self.busy() || self.processing() {
            return;
        }
        self.preferences.apply_budget();
        let settings = self.preferences.settings.clone();
        self.filter = Filter::All;
        self.work(ctx, "Preparing conversion…", move |engine| {
            engine.start(settings)?;
            Ok(engine.snapshot())
        });
    }

    fn queue_action(&mut self, ctx: &egui::Context, action: JobAction) {
        self.work(ctx, "Updating queue…", move |engine| {
            match action {
                JobAction::Cancel(id) => engine.cancel(id)?,
                JobAction::Retry(id) => engine.retry(id)?,
                JobAction::Remove(id) => engine.remove(id)?,
                JobAction::Clear => engine.clear_finished()?,
                JobAction::Reveal(path) => {
                    if !Path::new(&path).is_file() {
                        return Err("The output file was moved or deleted.".into());
                    }
                    platform::reveal(Path::new(&path)).map_err(|e| e.to_string())?;
                }
            }
            Ok(engine.snapshot())
        });
    }

    fn start_problem(&self) -> Option<String> {
        if !self.snapshot.jobs.iter().any(|job| job.status == "queued") {
            return Some("Add a file to the queue.".into());
        }
        if self.preferences.settings.output_dir.is_empty() {
            return Some("Choose an output folder.".into());
        }
        self.budget_problem()
    }

    fn budget_max(&self) -> Option<u64> {
        self.snapshot
            .jobs
            .iter()
            .filter(|job| ["queued", "running"].contains(&job.status.as_str()) && job.size > 0)
            .map(|job| job.size)
            .min()
    }

    fn budget_problem(&self) -> Option<String> {
        if !self.preferences.budget_enabled {
            return None;
        }
        let target = self.preferences.target();
        if target.is_none() {
            return Some("Enter a valid size of at least 0.001 MB.".into());
        }
        if self
            .budget_max()
            .is_some_and(|maximum| target.unwrap_or(0) > maximum)
        {
            return Some("The limit exceeds the smallest input. Lower it to compress every file in this batch.".into());
        }
        None
    }

    fn tick(&mut self, ctx: &egui::Context) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                WorkerMessage::Finished(result) => {
                    self.operation = None;
                    match result {
                        Ok(snapshot) => self.snapshot = snapshot,
                        Err(error) => self.error = Some(error),
                    }
                }
                WorkerMessage::Output(path) => {
                    self.operation = None;
                    if let Some(path) = path.filter(|_| !self.closing) {
                        self.preferences.settings.output_dir = path.to_string_lossy().into_owned();
                        self.changed(ctx);
                    }
                }
                WorkerMessage::PreferencesSaved(result) => {
                    self.saving = false;
                    if let Err(error) = result {
                        self.notice = Some(format!("Preferences could not be saved: {error}"));
                    }
                }
                WorkerMessage::Preview(generation, key, result) => {
                    store_preview(
                        &mut self.previews,
                        self.preview_generation,
                        generation,
                        key,
                        result,
                    );
                }
                WorkerMessage::Sources(result) => match result {
                    Ok(_) => self.snapshot = self.engine.snapshot(),
                    Err(error) if !self.closing => {
                        self.notice = Some(format!("Source inspection: {error}"))
                    }
                    Err(_) => {}
                },
                WorkerMessage::Closed => {
                    self.close_ready = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if Instant::now() >= self.next_snapshot {
            self.snapshot = self.engine.snapshot();
            self.next_snapshot = Instant::now() + Duration::from_millis(500);
        }
        if self.save_at.is_some_and(|when| Instant::now() >= when) && !self.saving && !self.closing
        {
            self.save_at = None;
            self.saving = true;
            self.preference_revision += 1;
            let revision = self.preference_revision;
            let writer = self.preference_writer.clone();
            let preferences = self.preferences.clone();
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(WorkerMessage::PreferencesSaved(
                    writer.save(revision, &preferences),
                ));
                ctx.request_repaint();
            });
        }
        if self.processing()
            || self.busy()
            || self.saving
            || self
                .preview_worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
        {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        if let Some(when) = self.save_at {
            ctx.request_repaint_after(when.saturating_duration_since(Instant::now()));
        }
        if ctx.input(|input| input.viewport().close_requested()) && !self.close_ready {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if !self.closing {
                self.closing = true;
                let preview = self.take_preview_worker();
                let sources = self.take_source_worker();
                self.preference_revision += 1;
                let revision = self.preference_revision;
                let writer = self.preference_writer.clone();
                let engine = self.engine.clone();
                let preferences = self.preferences.clone();
                let tx = self.tx.clone();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    if let Some(preview) = preview {
                        let _ = preview.join();
                    }
                    engine.shutdown();
                    if let Some(sources) = sources {
                        let _ = sources.join();
                    }
                    let _ = writer.save(revision, &preferences);
                    let _ = tx.send(WorkerMessage::Closed);
                    ctx.request_repaint();
                });
            }
        }
        if self.snapshot.jobs.iter().any(|job| job.status == "queued") {
            ctx.request_repaint_after(Duration::from_secs(2));
        }
        self.refresh_source_metadata(ctx);
        self.refresh_previews(ctx);
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.visuals_mut().override_text_color = Some(Color32::from_rgb(201, 215, 221));
        ui.visuals_mut().widgets.inactive.bg_fill = NAVY;
        ui.visuals_mut().widgets.hovered.bg_fill = Color32::from_rgb(45, 65, 79);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new((logo.id(), Vec2::splat(45.0))));
            }
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("NOVA")
                        .size(21.0)
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.label(
                    RichText::new("F L U X")
                        .size(12.0)
                        .color(Color32::from_rgb(164, 183, 195)),
                );
            });
        });
        ui.add_space(23.0);
        ui.label(
            RichText::new("YOUR CONVERSION STUDIO")
                .size(8.5)
                .color(Color32::from_rgb(123, 147, 161)),
        );
        ui.add_space(29.0);
        let count = self.snapshot.jobs.len();
        let queue =
            egui::Button::new(RichText::new(format!("Conversion queue    {count}")).size(11.0))
                .fill(Color32::from_rgb(40, 59, 73))
                .stroke(Stroke::NONE)
                .min_size(Vec2::new(ui.available_width(), 40.0));
        if ui.add(queue).clicked() {
            self.filter = Filter::All;
        }
        let settings_label = if self.preferences.settings_open {
            "Output settings  ‹"
        } else {
            "Output settings  ›"
        };
        if ui
            .add_enabled(
                !self.closing,
                egui::Button::new(RichText::new(settings_label).size(11.0))
                    .selected(self.preferences.settings_open)
                    .frame(false)
                    .min_size(Vec2::new(ui.available_width(), 37.0)),
            )
            .on_hover_text(if self.preferences.settings_open {
                "Hide output settings and expand the queue"
            } else {
                "Show output settings"
            })
            .clicked()
        {
            self.preferences.settings_open = !self.preferences.settings_open;
            self.changed(ui.ctx());
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.label(
                RichText::new(format!("RUST  /  LOCAL  /  {}", env!("CARGO_PKG_VERSION")))
                    .size(8.0)
                    .color(Color32::from_rgb(115, 142, 158)),
            );
            ui.add_space(12.0);
            if ui
                .add_enabled(
                    !self.closing,
                    egui::Button::new(if self.preferences.dark {
                        "Light appearance"
                    } else {
                        "Dark appearance"
                    }),
                )
                .clicked()
            {
                self.preferences.dark = !self.preferences.dark;
                apply_style(ui.ctx(), self.preferences.dark);
                self.changed(ui.ctx());
            }
            ui.add_space(25.0);
            ui.label(
                RichText::new("Local conversion. No uploads.")
                    .size(9.0)
                    .color(Color32::from_rgb(127, 153, 167)),
            );
            ui.label(
                RichText::new("Your device. Always.")
                    .size(14.0)
                    .color(Color32::from_rgb(233, 237, 230)),
            );
            ui.label(RichText::new("Your files.").size(14.0).color(CORAL));
        });
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("LESS FRICTION. MORE FLOW.")
                        .size(9.0)
                        .color(self.muted()),
                );
                ui.add_space(3.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Convert with ease").size(31.0).strong());
                    ui.label(RichText::new(".").size(36.0).color(CORAL));
                });
                ui.label(
                    RichText::new("A file or a whole folder. Ready for a fresh format.")
                        .size(11.0)
                        .color(self.muted()),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !self.busy() && !self.processing(),
                        egui::Button::new(RichText::new("+  Add files").color(Color32::WHITE))
                            .fill(NAVY)
                            .min_size(Vec2::new(97.0, 36.0)),
                    )
                    .clicked()
                {
                    self.pick_inputs(ui.ctx(), false);
                }
                if ui
                    .add_enabled(
                        !self.busy() && !self.processing(),
                        egui::Button::new("Add folder").min_size(Vec2::new(87.0, 36.0)),
                    )
                    .clicked()
                {
                    self.pick_inputs(ui.ctx(), true);
                }
            });
        });
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        let queued = self
            .snapshot
            .jobs
            .iter()
            .filter(|job| job.status == "queued")
            .count();
        let done = self
            .snapshot
            .jobs
            .iter()
            .filter(|job| job.status == "done")
            .count();
        let failed = self
            .snapshot
            .jobs
            .iter()
            .filter(|job| job.status == "error")
            .count();
        let headline = if self.closing {
            "Finishing safely…".into()
        } else if let Some(operation) = &self.operation {
            operation.clone()
        } else if self.processing() {
            "Conversion in progress".into()
        } else if queued > 0 {
            format!(
                "{queued} {} ready to convert",
                if queued == 1 { "file" } else { "files" }
            )
        } else if done > 0 {
            "Your output is ready".into()
        } else {
            "Ready when you are".into()
        };
        ui.horizontal(|ui| {
            let text_width =
                (ui.available_width() - if self.processing() { 270.0 } else { 190.0 }).max(130.0);
            ui.vertical(|ui| {
                ui.set_max_width(text_width);
                ui.label(RichText::new(headline).strong().size(13.0));
                let detail = if self.processing() {
                    format!("{done} completed · {queued} queued · {failed} failed")
                } else if let Some(problem) = self.start_problem() {
                    problem
                } else if self.preferences.settings.concurrency == 0 {
                    "Automatic resource scheduling · full duration preserved".into()
                } else {
                    format!(
                        "{} output · {} files at a time",
                        self.preferences.settings.format.to_uppercase(),
                        self.preferences.settings.concurrency
                    )
                };
                ui.add(
                    egui::Label::new(RichText::new(detail).size(10.0).color(self.muted()))
                        .truncate(),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let enabled = !self.busy() && !self.processing() && self.start_problem().is_none();
                let label = if self.processing() {
                    "Converting…"
                } else {
                    "Start converting  →"
                };
                let response = ui.add_enabled(
                    enabled,
                    egui::Button::new(RichText::new(label).strong().color(NAVY))
                        .fill(CORAL)
                        .stroke(Stroke::NONE)
                        .min_size(Vec2::new(167.0, 43.0)),
                );
                if response.clicked() {
                    self.start(ui.ctx());
                }
                if let Some(problem) = self.start_problem() {
                    response.on_hover_text(problem);
                }
                if self.processing()
                    && ui
                        .add_enabled(
                            !self.busy(),
                            egui::Button::new("Stop all").min_size(Vec2::new(69.0, 38.0)),
                        )
                        .clicked()
                {
                    self.queue_action(ui.ctx(), JobAction::Cancel(None));
                }
            });
        });
    }

    fn muted(&self) -> Color32 {
        if self.preferences.dark {
            Color32::from_rgb(156, 173, 180)
        } else {
            Color32::from_rgb(112, 125, 125)
        }
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "02 /", "Your output");
        ui.add_space(13.0);
        let enabled = !self.busy() && !self.processing();
        let before = serde_json::to_string(&self.preferences).unwrap_or_default();
        egui::ScrollArea::vertical().id_salt("settings-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                setting_label(ui, "Output format");
                let mut audio = AUDIO.contains(&self.preferences.settings.format.as_str());
                ui.horizontal(|ui| {
                    if ui.selectable_label(!audio, "Video").clicked() { audio = false; if !VIDEO.contains(&self.preferences.settings.format.as_str()) { self.preferences.settings.format = "mp4".into(); } }
                    if ui.selectable_label(audio, "Audio").clicked() { audio = true; if !AUDIO.contains(&self.preferences.settings.format.as_str()) { self.preferences.settings.format = "mp3".into(); } }
                });
                let formats = if audio { AUDIO } else { VIDEO };
                egui::Grid::new("output-format-grid").num_columns(3).spacing([6.0, 6.0]).show(ui, |ui| {
                    for (index, format) in formats.iter().enumerate() {
                        let compatible = !self.preferences.budget_enabled || !["wav", "flac"].contains(format);
                        let selected = self.preferences.settings.format == *format;
                        let button = egui::Button::new(format.to_uppercase()).selected(selected).min_size(Vec2::new(69.0, 29.0));
                        if ui.add_enabled(compatible, button).clicked() { self.preferences.settings.format = (*format).into(); }
                        if index % 3 == 2 { ui.end_row(); }
                    }
                });
                ui.add_space(17.0);
                self.quality_ui(ui);
                if !audio {
                    ui.add_space(16.0);
                    self.resolution_ui(ui);
                }
                ui.add_space(16.0);
                self.budget_ui(ui);
                ui.add_space(17.0);
                setting_label(ui, "Save to");
                let path = self.preferences.settings.output_dir.clone();
                let label = if path.is_empty() { "Choose output folder".into() } else { Path::new(&path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone()) };
                let response = ui.add_sized([ui.available_width(), 36.0], egui::Button::new(format!("Folder  /  {label}")));
                if response.clicked() { self.pick_output(ui.ctx()); }
                if !path.is_empty() { response.on_hover_text(&path); }
                ui.add_space(16.0);
                setting_label(ui, "Encoding mode");
                let auto_label = if cfg!(target_os = "macos") { "Automatic · Apple hardware + CPU" } else { "Automatic · compatible encoders" };
                let mode_text = if self.preferences.settings.performance == "cpu" { "CPU · software encoding" } else { auto_label };
                egui::ComboBox::from_id_salt("performance-mode").selected_text(mode_text).width(ui.available_width()).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.preferences.settings.performance, "auto".into(), auto_label);
                    ui.selectable_value(&mut self.preferences.settings.performance, "cpu".into(), "CPU · software encoding");
                });
                ui.add_space(12.0);
                setting_label(ui, "Parallel conversions");
                let concurrency_text = if self.preferences.settings.concurrency == 0 { "Automatic · CPU and RAM budget".into() } else { format!("{} {} at a time", self.preferences.settings.concurrency, if self.preferences.settings.concurrency == 1 { "file" } else { "files" }) };
                egui::ComboBox::from_id_salt("parallel-conversions").selected_text(concurrency_text).width(ui.available_width()).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.preferences.settings.concurrency, 0, "Automatic · CPU and RAM budget");
                    for count in [1, 2, 3, 5, 10] { ui.selectable_value(&mut self.preferences.settings.concurrency, count, format!("{count} {} at a time", if count == 1 { "file" } else { "files" })); }
                });
                small_note(ui, "Benchmarks encoders on this device, reuses calibration, and adapts work to CPU and available RAM. Size limits stay verified.", self.muted());
                if self.preferences.settings.quality == "original" {
                    ui.add_space(13.0);
                    small_note(ui, "Stream copy requires codecs compatible with the output container. It never silently re-encodes media.", CORAL);
                }
                ui.add_space(18.0);
            });
        });
        if before != serde_json::to_string(&self.preferences).unwrap_or_default() {
            self.changed(ui.ctx());
        }
    }

    fn quality_ui(&mut self, ui: &mut egui::Ui) {
        setting_label(ui, "Conversion quality");
        if self.preferences.budget_enabled {
            ui.label(RichText::new("Automatic for size limit").strong());
            small_note(ui, "The encoder adjusts quality to fit the maximum size below. Quality presets do not apply in this mode.", self.muted());
            return;
        }
        egui::Grid::new("quality-grid")
            .num_columns(2)
            .spacing([6.0, 6.0])
            .show(ui, |ui| {
                for (index, (value, label)) in QUALITY.iter().enumerate() {
                    let (size, detail) =
                        self.estimate_preview(value, self.preferences.settings.resolution);
                    let button = egui::Button::new(format!("{label}\n{size}"))
                        .selected(self.preferences.settings.quality == *value)
                        .min_size(Vec2::new(106.0, 46.0));
                    if ui.add(button).on_hover_text(detail).clicked() {
                        self.preferences.settings.quality = (*value).into();
                        self.preferences.ordinary_quality = (*value).into();
                        if *value == "original" {
                            self.preferences.settings.resolution = None;
                        }
                    }
                    if index % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        let count = self
            .snapshot
            .jobs
            .iter()
            .filter(|job| job.status == "queued")
            .count();
        if count == 0 {
            small_note(ui, "Add files to measure size estimates.", self.muted());
        } else {
            let measuring = self
                .preview_worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished());
            if VIDEO.contains(&self.preferences.settings.format.as_str()) {
                small_note(ui, "Compact: up to 40% of source size · Balanced: 65% · High quality: 85%. Verified maximums at original dimensions; final files can be smaller. Compression may reduce visible quality and audio fidelity. Stream copy is measured separately.", self.muted());
            } else {
                small_note(ui, &format!("{} · total for {count} queued {}. Complete-file size can differ from sampled content.", if measuring { "Measuring local samples" } else { "Sample-based estimates" }, if count == 1 { "file" } else { "files" }), self.muted());
            }
            if ui.small_button("Recheck estimates").clicked() {
                self.invalidate_previews();
                self.previews.clear();
            }
        }
    }

    fn estimate_preview(&self, quality: &str, resolution: Option<u32>) -> (String, String) {
        if self.preferences.budget_enabled {
            return ("Automatic for limit".into(), "The maximum size is shown once in Target file size. It is not a resolution estimate.".into());
        }
        let jobs: Vec<&Job> = self
            .snapshot
            .jobs
            .iter()
            .filter(|job| job.status == "queued")
            .collect();
        if jobs.is_empty() {
            return (
                "Add files".into(),
                "Add media to measure a size estimate.".into(),
            );
        }
        let mut settings = self.preferences.settings.clone();
        settings.target_bytes = None;
        settings.quality = quality.into();
        settings.resolution = if quality == "original" {
            None
        } else {
            resolution
        };
        let preset_caps: Option<Vec<u64>> = jobs
            .iter()
            .map(|job| output_profile::preset_limit(job, &settings))
            .collect();
        if let Some(caps) = preset_caps {
            if let Some(total) = caps.iter().try_fold(0u64, |sum, cap| sum.checked_add(*cap)) {
                let ratio = match quality {
                    "low" => 40,
                    "high" => 85,
                    _ => 65,
                };
                return (format!("≤ {} max", limit_size_text(total)), format!("Source-aware compression: {ratio}% of each input file. Total maximum for {} queued file(s): {} bytes. This is an enforced ceiling, not a sampled size prediction. The completed output is checked before saving; impossible compression returns an error. Original dimensions, cadence and selected timeline are preserved.", jobs.len(), grouped_bytes(total)));
            }
            return (
                "Unavailable".into(),
                "The complete queue maximum exceeds the supported numeric range.".into(),
            );
        }
        let mut estimates = Vec::new();
        let mut evidence = Vec::new();
        let context = preview_context(jobs.iter().copied());
        for job in &jobs {
            match self.previews.get(&preview_key(job, &settings, context)) {
                Some(Ok(measured)) => {
                    estimates.push(Some(measured.estimate.clone()));
                    evidence.push(format!(
                        "{}: {} measurement(s), {:.2} seconds processed, encoder {}. {}",
                        job.name,
                        measured.samples,
                        measured.sampled_seconds,
                        measured.encoder,
                        measured.estimate.method
                    ));
                }
                Some(Err(error)) => return ("Unavailable".into(), error.clone()),
                None => {
                    return (
                        "Measuring…".into(),
                        if quality == "original" {
                            "Measuring one complete private stream-copy remux without seeking or trimming. It is cancellable and limited to 15 seconds; no guessed number is substituted.".into()
                        } else {
                            "Encoding short local samples with this output profile. Conversion can start at any time; measurements are cancelled first. No guessed number is substituted.".into()
                        },
                    )
                }
            }
        }
        let Some(total) = combine_estimates(&estimates) else {
            return (
                "Unavailable".into(),
                "A complete queue measurement is unavailable. Partial totals are not displayed."
                    .into(),
            );
        };
        if quality == "original" {
            return (
                format!("≈ {}", size_text(total.bytes)),
                format!(
                    "Measured complete remux total for {} file(s): {} bytes. Every selected timeline was copied privately without seeking, trimming or extrapolating samples. Final container bytes are checked after conversion.\n{}",
                    jobs.len(), grouped_bytes(total.bytes), evidence.join("\n")
                ),
            );
        }
        (format!("≈ {}", size_text(total.bytes)), format!("Sample-based total for {} file(s): approximately {} bytes.\nObserved sample spread projected to the full duration: {}–{}. This is not a guaranteed range; scenes outside the samples may differ. Automatic may choose or fall back to another encoder during conversion. Exact file bytes are measured after conversion.\n{}", jobs.len(), grouped_bytes(total.bytes), size_text(total.lower_bytes), size_text(total.upper_bytes), evidence.join("\n")))
    }

    fn resolution_ui(&mut self, ui: &mut egui::Ui) {
        setting_label(ui, "Video dimensions");
        let dimensions: HashSet<_> = self
            .snapshot
            .jobs
            .iter()
            .filter(|job| {
                ["queued", "running"].contains(&job.status.as_str()) && job.video.is_some()
            })
            .filter_map(|job| Some((job.width?, job.height?)))
            .collect();
        let text = if dimensions.len() == 1 {
            let (width, height) = *dimensions.iter().next().unwrap();
            format!("Original · {width}×{height}")
        } else if dimensions.is_empty() {
            "Original · add files".into()
        } else {
            "Original dimensions of each file".into()
        };
        ui.label(RichText::new(text).strong());
        small_note(ui, "Locked to the source. No resizing, padding or frame-rate reduction. Only encoding quality changes.", self.muted());
    }

    fn budget_ui(&mut self, ui: &mut egui::Ui) {
        let stroke = Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
        egui::Frame::NONE.fill(ui.visuals().faint_bg_color).stroke(stroke).corner_radius(7).inner_margin(12).show(ui, |ui| {
            let was_enabled = self.preferences.budget_enabled;
            ui.checkbox(&mut self.preferences.budget_enabled, RichText::new("Target file size").strong());
            small_note(ui, "Hard maximum per output file. Verified before saving.", self.muted());
            if was_enabled != self.preferences.budget_enabled {
                if self.preferences.budget_enabled { self.preferences.ordinary_quality = self.preferences.settings.quality.clone(); self.preferences.apply_budget(); }
                else { self.preferences.settings.quality = self.preferences.ordinary_quality.clone(); self.preferences.apply_budget(); }
            }
            if !self.preferences.budget_enabled { return; }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Limit").size(11.0));
                ui.add(egui::TextEdit::singleline(&mut self.preferences.budget_draft).desired_width(112.0).hint_text("1.0").char_limit(64));
                ui.label("MB");
            });
            let maximum = self.budget_max();
            let (minimum, max_mb, slider_enabled) = slider_bounds(maximum);
            let mut value = self.preferences.target().map(|bytes| bytes as f64 / 1_000_000.0).unwrap_or(1.0).clamp(minimum.max(f64::MIN_POSITIVE), max_mb.max(minimum));
            ui.add_enabled_ui(slider_enabled, |ui| {
                ui.spacing_mut().slider_width = (ui.available_width() - 6.0).max(70.0);
                if ui.add(egui::Slider::new(&mut value, minimum..=max_mb.max(minimum)).logarithmic(true).show_value(false)).on_hover_text("Drag to choose a size from the source size down to 0.001 MB.").changed() {
                    self.preferences.budget_draft = decimal_mb(value);
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("0.001 MB").size(9.0).color(self.muted()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(maximum.map(size_text).unwrap_or_else(|| "Add a file".into())).size(9.0).color(self.muted()));
                });
            });
            ui.horizontal(|ui| {
                if ui.small_button("1 MB").clicked() { self.preferences.budget_draft = "1".into(); }
                if ui.small_button("5 MB").clicked() { self.preferences.budget_draft = "5".into(); }
            });
            small_note(ui, "1 MB = 1,000,000 bytes. The slider maximum follows the smallest queued input.", self.muted());
            if let Some(problem) = self.budget_problem() { small_note(ui, &problem, Color32::from_rgb(190, 79, 67)); }
            if let Some(target) = self.preferences.target() {
                small_note(ui, &format!("{} bytes per output file", grouped_bytes(target)), self.muted());
            }
            small_note(ui, "Original dimensions, frame cadence and full duration are preserved. If quality changes cannot fit the limit, no output is saved.", self.muted());
            small_note(ui, "Stream copy, WAV and FLAC are unavailable with a size target.", self.muted());
        });
    }

    fn queue(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "01 /", "Your files");
        ui.add_space(13.0);
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            for (filter, label) in [
                (Filter::All, "All"),
                (Filter::Active, "Active"),
                (Filter::Done, "Completed"),
                (Filter::Attention, "Needs attention"),
            ] {
                if ui.selectable_label(self.filter == filter, label).clicked() {
                    self.filter = filter;
                }
            }
            let completed = self.snapshot.jobs.iter().any(|job| job.status == "done");
            if ui
                .add_enabled(
                    !self.busy() && completed,
                    egui::Button::new("Clear completed").small(),
                )
                .clicked()
            {
                action = Some(JobAction::Clear);
            }
        });
        ui.add_space(8.0);
        ui.separator();
        if let Some(error) = self.error.clone() {
            message(ui, &error, true, self.preferences.dark, &mut self.error);
        }
        if let Some(notice) = self.notice.clone() {
            message(ui, &notice, false, self.preferences.dark, &mut self.notice);
        }
        if !self.snapshot.warnings.is_empty() {
            egui::CollapsingHeader::new(format!("{} input notices", self.snapshot.warnings.len()))
                .id_salt("input-warnings")
                .show(ui, |ui| {
                    for warning in &self.snapshot.warnings {
                        ui.add(
                            egui::Label::new(RichText::new(warning).size(11.0).color(CORAL)).wrap(),
                        );
                    }
                });
        }
        let available = (ui.available_height() - 36.0).max(80.0);
        egui::ScrollArea::vertical()
            .id_salt("queue-scroll")
            .auto_shrink([false, false])
            .max_height(available)
            .show(ui, |ui| {
                let mut shown = 0;
                for job in &self.snapshot.jobs {
                    let show = match self.filter {
                        Filter::All => true,
                        Filter::Active => ["queued", "running"].contains(&job.status.as_str()),
                        Filter::Done => job.status == "done",
                        Filter::Attention => ["error", "cancelled"].contains(&job.status.as_str()),
                    };
                    if show {
                        shown += 1;
                        job_row(ui, job, !self.busy(), self.muted(), &mut action);
                    }
                }
                if shown == 0 {
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), available - 10.0),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.add_space((available * 0.20).max(26.0));
                            draw_empty_mark(ui);
                            ui.add_space(20.0);
                            ui.label(
                                RichText::new(if self.filter == Filter::All {
                                    "A fresh start, right here"
                                } else {
                                    "Nothing in this view"
                                })
                                .size(20.0)
                                .strong(),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new("Drop audio and video files here,")
                                    .size(12.0)
                                    .color(self.muted()),
                            );
                            ui.label(
                                RichText::new("or add them to the queue with a click.")
                                    .size(12.0)
                                    .color(self.muted()),
                            );
                            ui.add_space(14.0);
                            if ui
                                .add_enabled(
                                    !self.busy() && !self.processing(),
                                    egui::Button::new("Choose files").fill(CORAL),
                                )
                                .clicked()
                            {
                                self.pick_inputs(ui.ctx(), false);
                            }
                            ui.add_space(17.0);
                            ui.label(
                                RichText::new("MP4 · MKV · MOV · MP3 · WAV & MORE")
                                    .size(9.0)
                                    .color(self.muted()),
                            );
                        },
                    );
                }
            });
        ui.add_space(8.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("●  Processed on this device")
                    .size(10.0)
                    .color(self.muted()),
            );
            let bytes = self
                .snapshot
                .jobs
                .iter()
                .try_fold(0_u64, |total, job| total.checked_add(job.size));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(match bytes {
                        Some(bytes) if bytes > 0 => format!("Total input: {}", size_text(bytes)),
                        Some(_) => "Original files stay untouched".into(),
                        None => "Total exceeds supported byte count".into(),
                    })
                    .size(10.0)
                    .color(self.muted()),
                );
            });
        });
        if let Some(action) = action {
            self.queue_action(ui.ctx(), action);
        }
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        if let Some(worker) = self.take_preview_worker() {
            let _ = worker.join();
        }
        if let Some(worker) = self.take_source_worker() {
            self.engine.shutdown();
            let _ = worker.join();
        }
    }
}

impl eframe::App for Desktop {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.tick(&ctx);
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() && !self.busy() && !self.processing() {
            self.work(&ctx, "Reading dropped files…", move |engine| {
                engine.import(dropped)
            });
            self.filter = Filter::All;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::O))
            && !self.busy()
            && !self.processing()
        {
            self.pick_inputs(&ctx, false);
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
            self.start(&ctx);
        }
        let surface = if self.preferences.dark {
            Color32::from_rgb(30, 43, 51)
        } else {
            Color32::from_rgb(246, 245, 236)
        };
        egui::Panel::left("navigation")
            .exact_size(180.0)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(NAVY).inner_margin(20))
            .show(ui, |ui| self.sidebar(ui));
        egui::Panel::top("header")
            .exact_size(133.0)
            .frame(egui::Frame::NONE.fill(surface).inner_margin(egui::Margin {
                left: 28,
                right: 28,
                top: 24,
                bottom: 13,
            }))
            .show(ui, |ui| self.header(ui));
        egui::Panel::bottom("conversion-footer")
            .exact_size(88.0)
            .frame(
                egui::Frame::NONE
                    .fill(surface)
                    .inner_margin(egui::Margin {
                        left: 28,
                        right: 28,
                        top: 18,
                        bottom: 18,
                    })
                    .stroke(Stroke::new(
                        1.0,
                        ui.visuals().widgets.noninteractive.bg_stroke.color,
                    )),
            )
            .show(ui, |ui| self.footer(ui));
        if self.preferences.settings_open {
            egui::Panel::right("output-settings")
                .exact_size(282.0)
                .resizable(false)
                .frame(egui::Frame::NONE.fill(surface).inner_margin(egui::Margin {
                    left: 20,
                    right: 25,
                    top: 8,
                    bottom: 10,
                }))
                .show(ui, |ui| self.settings(ui));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(surface).inner_margin(egui::Margin {
                left: 28,
                right: 24,
                top: 8,
                bottom: 12,
            }))
            .show(ui, |ui| self.queue(ui));
        let hovered = ctx.input(|input| !input.raw.hovered_files.is_empty());
        if let Some(smoke) = &mut self.smoke {
            smoke.update(&ctx);
        }
        if hovered {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("file-drop-overlay"),
            ));
            let rect = ctx.viewport_rect().shrink(16.0);
            painter.rect_filled(rect, 14, Color32::from_rgba_unmultiplied(20, 37, 50, 245));
            painter.rect_stroke(rect, 14, Stroke::new(2.0, CORAL), egui::StrokeKind::Inside);
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                if self.processing() {
                    "Finish the current conversion, then drop files."
                } else if self.busy() {
                    "Finish the current action, then drop files."
                } else {
                    "Drop it. We’ll queue it."
                },
                FontId::proportional(28.0),
                Color32::WHITE,
            );
        }
    }
}

enum JobAction {
    Cancel(Option<String>),
    Retry(String),
    Remove(String),
    Reveal(String),
    Clear,
}

fn job_row(
    ui: &mut egui::Ui,
    job: &Job,
    enabled: bool,
    muted: Color32,
    action: &mut Option<JobAction>,
) {
    ui.push_id(&job.id, |ui| {
        ui.add_space(12.0);
        ui.horizontal_top(|ui| {
            let color = if job.video.is_some() { GREEN } else { CORAL };
            let badge = if job.video.is_some() {
                "VIDEO"
            } else {
                "AUDIO"
            };
            egui::Frame::NONE
                .fill(color.gamma_multiply(0.12))
                .corner_radius(5)
                .inner_margin(8)
                .show(ui, |ui| {
                    ui.label(RichText::new(badge).size(8.0).strong().color(color));
                });
            let button_width = match job.status.as_str() {
                "running" | "queued" => 54.0,
                "error" | "cancelled" => 100.0,
                _ => 112.0,
            };
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - button_width - 14.0).max(80.0));
                ui.add(egui::Label::new(RichText::new(&job.name).size(12.0).strong()).truncate())
                    .on_hover_text(&job.path);
                let dimensions = match (job.width, job.height) {
                    (Some(width), Some(height)) => format!(" · {width}×{height}"),
                    _ => String::new(),
                };
                ui.label(
                    RichText::new(format!(
                        "{} · {}{dimensions}",
                        size_text(job.size),
                        duration_text(job.duration)
                    ))
                    .size(10.0)
                    .color(muted),
                );
                ui.label(RichText::new(format!("{} source bytes", grouped_bytes(job.size)))
                    .size(9.0).color(muted))
                    .on_hover_text(format!("{} bytes = {:.6} MB = {:.6} MiB\nMB = 1,000,000 bytes; MiB = 1,048,576 bytes.\n{}",
                        grouped_bytes(job.size), job.size as f64 / 1_000_000.0,
                        job.size as f64 / 1_048_576.0, job.path));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.add_enabled_ui(enabled, |ui| match job.status.as_str() {
                    "running" | "queued" => {
                        if ui.small_button("Cancel").clicked() {
                            *action = Some(JobAction::Cancel(Some(job.id.clone())));
                        }
                    }
                    "error" | "cancelled" => {
                        if ui.small_button("Remove").clicked() {
                            *action = Some(JobAction::Remove(job.id.clone()));
                        }
                        if ui.small_button("Retry").clicked() {
                            *action = Some(JobAction::Retry(job.id.clone()));
                        }
                    }
                    "done" => {
                        if ui.small_button("Remove").clicked() {
                            *action = Some(JobAction::Remove(job.id.clone()));
                        }
                        if let Some(output) = &job.output {
                            if ui.small_button("Show file").clicked() {
                                *action = Some(JobAction::Reveal(output.clone()));
                            }
                        }
                    }
                    _ => {}
                });
            });
        });
        ui.add_space(7.0);
        let status_color = match job.status.as_str() {
            "done" => GREEN,
            "error" => Color32::from_rgb(191, 82, 70),
            "running" => CORAL,
            _ => muted,
        };
        let status = match job.status.as_str() {
            "running" => job.stage.as_deref().unwrap_or("Converting"),
            "done" => "Saved successfully",
            "error" => "Conversion failed",
            "cancelled" => "Cancelled",
            _ => "Waiting to convert",
        };
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(RichText::new(status).size(10.0).color(status_color)).wrap());
            if job.status == "running" {
                ui.label(
                    RichText::new(format!("{:.0}%", job.progress.clamp(0.0, 100.0)))
                        .size(10.0)
                        .color(CORAL),
                );
            }
        });
        if job.status == "running" {
            ui.add(
                egui::ProgressBar::new((job.progress / 100.0).clamp(0.0, 1.0) as f32)
                    .desired_height(4.0)
                    .fill(CORAL),
            );
        }
        if let Some(output_size) = job.output_size {
            let reduction = if job.size > 0 {
                100.0 * (1.0 - output_size as f64 / job.size as f64)
            } else {
                0.0
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("Output {}", size_text(output_size)))
                        .size(10.0)
                        .strong(),
                )
                .on_hover_text(format!("{} bytes", grouped_bytes(output_size)));
                if reduction > 0.0 {
                    ui.label(
                        RichText::new(format!("{reduction:.1}% smaller"))
                            .size(10.0)
                            .color(GREEN),
                    );
                } else if reduction < 0.0 {
                    ui.label(RichText::new(format!("{:.1}% larger", -reduction))
                        .size(10.0).color(muted));
                } else {
                    ui.label(RichText::new("Same byte size").size(10.0).color(muted));
                }
                if let Some(limit) = job.target_bytes {
                    ui.label(
                        RichText::new(format!("{} {}", if job.target_is_preset { "Preset maximum" } else { "Maximum" }, limit_size_text(limit)))
                            .size(10.0)
                            .color(muted),
                    )
                    .on_hover_text(format!("{limit} bytes"));
                }
            });
            ui.label(RichText::new(format!("{} output bytes", grouped_bytes(output_size)))
                .size(9.0).color(muted));
            if let (Some(width), Some(height)) = (job.output_width, job.output_height) {
                let seconds = job.output_duration.map(|value| format!(" · {value:.3} s")).unwrap_or_default();
                let fps = job.output_frame_rate.map(|value| format!(" · {value:.3} fps")).unwrap_or_default();
                ui.label(RichText::new(format!("Actual output: {width}×{height}{seconds}{fps}"))
                    .size(9.0).color(muted));
            } else if let Some(seconds) = job.output_duration {
                ui.label(RichText::new(format!("Actual output: {seconds:.3} s"))
                    .size(9.0).color(muted));
            }
        }
        if let Some(encoder) = &job.encoder {
            ui.label(
                RichText::new(format!("Encoder: {encoder}"))
                    .size(9.0)
                    .color(muted),
            );
        }
        if let Some(note) = &job.performance_note {
            ui.add(egui::Label::new(RichText::new(note).size(9.0).color(muted)).wrap());
        }
        if let Some(error) = &job.error {
            egui::CollapsingHeader::new(
                RichText::new("Error details")
                    .size(10.0)
                    .color(Color32::from_rgb(191, 82, 70)),
            )
            .default_open(true)
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(error)
                            .size(10.0)
                            .color(Color32::from_rgb(191, 82, 70)),
                    )
                    .wrap(),
                );
            });
        }
        ui.add_space(12.0);
        ui.separator();
    });
}

fn section_title(ui: &mut egui::Ui, number: &str, title: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(number).size(11.0).color(CORAL));
        ui.label(RichText::new(title).size(16.0).strong());
    });
}

fn setting_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).strong());
    ui.add_space(4.0);
}

fn small_note(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.add_space(5.0);
    ui.add(egui::Label::new(RichText::new(text).size(10.0).color(color)).wrap());
}

fn message(ui: &mut egui::Ui, text: &str, error: bool, dark: bool, current: &mut Option<String>) {
    let color = if error {
        Color32::from_rgb(190, 79, 67)
    } else {
        CORAL
    };
    egui::Frame::NONE
        .fill(if dark {
            Color32::from_rgb(54, 43, 43)
        } else {
            Color32::from_rgb(249, 232, 221)
        })
        .inner_margin(10)
        .corner_radius(5)
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.add(egui::Label::new(RichText::new(text).size(11.0).color(color)).wrap());
                if ui.small_button("Dismiss").clicked() {
                    *current = None;
                }
            });
        });
}

fn preview_context<'a>(jobs: impl Iterator<Item = &'a Job>) -> (usize, u64) {
    jobs.fold((0, 0), |(count, pixels), job| {
        (
            count + 1,
            pixels.max(job.width.unwrap_or(0) as u64 * job.height.unwrap_or(0) as u64),
        )
    })
}

fn preview_key(job: &Job, settings: &Settings, context: (usize, u64)) -> String {
    format!(
        "{}|{}|{}|{}|{}|{:?}|{}|{}|{:?}",
        job.id,
        job.source_revision,
        job.size,
        settings.format,
        settings.quality,
        settings.resolution,
        settings.performance,
        settings.concurrency,
        context
    )
}

fn store_preview(
    cache: &mut HashMap<String, Result<MeasuredEstimate, String>>,
    current_generation: u64,
    received_generation: u64,
    key: String,
    result: Result<MeasuredEstimate, String>,
) {
    if current_generation == received_generation {
        cache.insert(key, result);
    }
}

fn grouped_bytes(bytes: u64) -> String {
    let raw = bytes.to_string();
    let mut result = String::with_capacity(raw.len() + raw.len() / 3);
    for (index, digit) in raw.chars().enumerate() {
        if index > 0 && (raw.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(digit);
    }
    result
}

fn combine_estimates(estimates: &[Option<Estimate>]) -> Option<Estimate> {
    let first = estimates.first()?.as_ref()?;
    let mut total = Estimate {
        bytes: 0,
        lower_bytes: 0,
        upper_bytes: 0,
        method: first.method,
        is_limit: first.is_limit,
    };
    for estimate in estimates {
        let estimate = estimate.as_ref()?;
        total.bytes = total.bytes.checked_add(estimate.bytes)?;
        total.lower_bytes = total.lower_bytes.checked_add(estimate.lower_bytes)?;
        total.upper_bytes = total.upper_bytes.checked_add(estimate.upper_bytes)?;
        if estimate.is_limit != total.is_limit {
            return None;
        }
        if estimate.method != total.method {
            total.method = "Combined metadata models";
        }
    }
    Some(total)
}

fn limit_size_text(bytes: u64) -> String {
    // A displayed ceiling must never round below the exact byte ceiling.
    let (step, precision, unit) = if bytes < 1_000 {
        return format!("{bytes} B");
    } else if bytes < 1_000_000 {
        (100, 10, "KB")
    } else if bytes < 1_000_000_000 {
        (10_000, 100, "MB")
    } else {
        (10_000_000, 100, "GB")
    };
    let rounded = bytes.div_ceil(step);
    if precision == 10 {
        format!("{}.{:01} {unit}", rounded / precision, rounded % precision)
    } else {
        format!("{}.{:02} {unit}", rounded / precision, rounded % precision)
    }
}

fn size_text(bytes: u64) -> String {
    if bytes < 1_000 {
        format!("{bytes} B")
    } else if bytes < 1_000_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else if bytes < 1_000_000_000 {
        format!("{:.2} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{:.2} GB", bytes as f64 / 1_000_000_000.0)
    }
}

fn duration_text(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "Unknown duration".into();
    }
    let seconds = seconds.round() as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn decimal_mb(value: f64) -> String {
    let value = format!("{value:.6}");
    value.trim_end_matches('0').trim_end_matches('.').into()
}

fn slider_bounds(maximum: Option<u64>) -> (f64, f64, bool) {
    let max_mb = maximum
        .map(|bytes| bytes as f64 / 1_000_000.0)
        .unwrap_or(1.0);
    (
        0.001_f64.min(max_mb).max(f64::MIN_POSITIVE),
        max_mb.max(f64::MIN_POSITIVE),
        maximum.is_some() && max_mb >= 0.001,
    )
}

fn draw_empty_mark(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(150.0, 105.0), egui::Sense::hover());
    let painter = ui.painter();
    let center = rect.center();
    painter.circle_stroke(
        center,
        48.0,
        Stroke::new(1.0, Color32::from_rgb(204, 211, 199)),
    );
    painter.rect_filled(
        egui::Rect::from_center_size(center + Vec2::new(-17.0, 2.0), Vec2::new(53.0, 70.0)),
        7,
        Color32::from_rgb(216, 230, 218),
    );
    painter.rect_filled(
        egui::Rect::from_center_size(center + Vec2::new(17.0, -3.0), Vec2::new(53.0, 70.0)),
        7,
        Color32::from_rgb(249, 224, 204),
    );
    painter.text(
        center + Vec2::new(17.0, -3.0),
        egui::Align2::CENTER_CENTER,
        "▶",
        FontId::proportional(23.0),
        CORAL,
    );
    painter.circle_filled(center + Vec2::new(-56.0, -31.0), 4.0, CORAL);
    painter.circle_filled(center + Vec2::new(55.0, 33.0), 3.0, GREEN);
}

fn install_fonts(ctx: &egui::Context) {
    let mut definitions = egui::FontDefinitions::default();
    for (name, path, primary) in [
        ("mac-sans", "/System/Library/Fonts/SFNS.ttf", true),
        ("mac-arabic", "/System/Library/Fonts/GeezaPro.ttc", false),
        (
            "mac-unicode",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            false,
        ),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            definitions
                .font_data
                .insert(name.into(), Arc::new(egui::FontData::from_owned(bytes)));
            let family = definitions
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default();
            if primary {
                family.insert(0, name.into());
            } else {
                family.push(name.into());
            }
        }
    }
    ctx.set_fonts(definitions);
}

fn apply_style(ctx: &egui::Context, dark: bool) {
    let theme = egui::Theme::from_dark_mode(dark);
    ctx.set_theme(theme);
    let mut style = (*ctx.style_of(theme)).clone();
    style.visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    style.visuals.override_text_color = Some(if dark {
        Color32::from_rgb(236, 238, 229)
    } else {
        NAVY
    });
    style.visuals.selection.bg_fill = CORAL.gamma_multiply(0.3);
    style.visuals.selection.stroke = Stroke::new(1.0, CORAL);
    style.visuals.widgets.active.bg_fill = CORAL.gamma_multiply(0.4);
    style.visuals.widgets.hovered.bg_fill = if dark {
        Color32::from_rgb(58, 76, 83)
    } else {
        Color32::from_rgb(232, 232, 220)
    };
    style.visuals.widgets.inactive.bg_fill = if dark {
        Color32::from_rgb(43, 60, 68)
    } else {
        Color32::from_rgb(239, 239, 228)
    };
    style.visuals.widgets.inactive.corner_radius = 5.into();
    style.visuals.widgets.hovered.corner_radius = 5.into();
    style.visuals.widgets.active.corner_radius = 5.into();
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(
        1.0,
        if dark {
            Color32::from_rgb(65, 80, 87)
        } else {
            Color32::from_rgb(214, 218, 205)
        },
    );
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(9.0, 7.0);
    style.spacing.interact_size = Vec2::new(32.0, 29.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(12.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(11.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(10.0));
    ctx.set_style_of(theme, style);
}

pub fn run(runtime: PathBuf) -> eframe::Result {
    let engine = Arc::new(Engine::new(runtime));
    let app_engine = engine.clone();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Nova Flux")
        .with_inner_size([1180.0, 800.0])
        .with_min_inner_size([920.0, 650.0])
        .with_drag_and_drop(true);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(icon);
    }
    let mut wgpu_options = eframe::egui_wgpu::WgpuConfiguration::default();
    if std::env::var("NOVA_WGPU_FORCE_FALLBACK").is_ok_and(|value| value == "1") {
        if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut wgpu_options.wgpu_setup {
            setup.native_adapter_selector = Some(Arc::new(|adapters, surface| {
                adapters.iter().find(|adapter| adapter.get_info().device_type == eframe::wgpu::DeviceType::Cpu
                    && surface.is_none_or(|surface| adapter.is_surface_supported(surface)))
                    .cloned().ok_or_else(|| "A CPU software renderer was explicitly requested but none supports the native surface.".into())
            }));
        }
    }
    let result = eframe::run_native(
        "Nova Flux",
        eframe::NativeOptions {
            viewport,
            centered: true,
            renderer: eframe::Renderer::Wgpu,
            wgpu_options,
            ..Default::default()
        },
        Box::new(move |cc| Ok(Box::new(Desktop::new(cc, app_engine)))),
    );
    // Also handles window-system teardown paths which bypass the close event.
    let _ = std::thread::spawn(move || engine.shutdown()).join();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recheck_rejects_already_queued_results_from_the_previous_generation() {
        let mut cache = HashMap::new();
        store_preview(&mut cache, 4, 3, "profile".into(), Err("old result".into()));
        assert!(cache.is_empty());
        store_preview(
            &mut cache,
            4,
            4,
            "profile".into(),
            Err("current inspection".into()),
        );
        assert_eq!(
            cache.get("profile").unwrap().as_ref().unwrap_err(),
            "current inspection"
        );
        cache.clear();
        store_preview(
            &mut cache,
            5,
            4,
            "profile".into(),
            Err("delayed old result".into()),
        );
        assert!(cache.is_empty());
    }

    #[test]
    fn size_display_distinguishes_decimal_mb_from_exact_source_bytes() {
        let bytes = 266_873_590;
        assert_eq!(size_text(bytes), "266.87 MB");
        assert_eq!(grouped_bytes(bytes), "266,873,590");
        assert_eq!(grouped_bytes(u64::MAX), "18,446,744,073,709,551,615");
        assert_eq!(limit_size_text(1_234_567), "1.24 MB");
    }

    #[test]
    fn preview_total_includes_all_files_and_never_hides_an_unknown_or_overflow() {
        let value = |bytes| {
            Some(Estimate {
                bytes,
                lower_bytes: bytes / 2,
                upper_bytes: bytes * 2,
                method: "Model",
                is_limit: false,
            })
        };
        let total = combine_estimates(&[value(1_000_000), value(2_000_000)]).unwrap();
        assert_eq!(total.bytes, 3_000_000);
        assert_eq!(total.lower_bytes, 1_500_000);
        assert_eq!(total.upper_bytes, 6_000_000);
        assert!(combine_estimates(&[value(1_000_000), None]).is_none());
        assert!(combine_estimates(&[]).is_none());
        let huge = || {
            Some(Estimate {
                bytes: u64::MAX,
                lower_bytes: u64::MAX,
                upper_bytes: u64::MAX,
                method: "Model",
                is_limit: false,
            })
        };
        assert!(combine_estimates(&[huge(), huge()]).is_none());
    }

    #[test]
    fn leaving_target_mode_restores_copy_without_an_incompatible_resize() {
        let mut preferences = Preferences::default();
        preferences.settings.quality = "original".into();
        preferences.ordinary_quality = "original".into();
        preferences.budget_enabled = true;
        preferences.apply_budget();
        preferences.settings.resolution = Some(720);
        preferences.budget_enabled = false;
        preferences.settings.quality = preferences.ordinary_quality.clone();
        preferences.apply_budget();
        assert_eq!(preferences.settings.quality, "original");
        assert_eq!(preferences.settings.resolution, None);
    }

    #[test]
    fn legacy_resize_preferences_reset_to_original_without_changing_quality() {
        let mut data = serde_json::to_value(Preferences::default()).unwrap();
        data["settings"]["resolution"] = serde_json::json!(720);
        let mut preferences: Preferences = serde_json::from_value(data).unwrap();
        preferences.sanitize();
        assert!(serde_json::to_value(&preferences).unwrap()["settings"]["resolution"].is_null());
        assert_eq!(preferences.settings.quality, "medium");

        let mut data = serde_json::to_value(&preferences).unwrap();
        data["settings"]["resolution"] = serde_json::json!(999);
        preferences = serde_json::from_value(data).unwrap();
        preferences.sanitize();
        assert!(serde_json::to_value(&preferences).unwrap()["settings"]["resolution"].is_null());

        let mut data = serde_json::to_value(&preferences).unwrap();
        data["settings"]["resolution"] = serde_json::json!(480);
        data["settings"]["quality"] = serde_json::json!("original");
        preferences = serde_json::from_value(data).unwrap();
        preferences.sanitize();
        assert!(serde_json::to_value(&preferences).unwrap()["settings"]["resolution"].is_null());
    }

    #[test]
    fn target_input_uses_exact_decimal_bytes_and_rejects_invalid_bounds() {
        let mut preferences = Preferences::default();
        for (text, bytes) in [
            ("1", 1_000_000),
            ("0.001", 1_000),
            ("1.234567", 1_234_567),
            (" 5 ", 5_000_000),
        ] {
            preferences.budget_draft = text.into();
            assert_eq!(preferences.target(), Some(bytes));
        }
        for text in [
            "", "NaN", "inf", "-1", "0", "0.000999", "1099512", "no size",
        ] {
            preferences.budget_draft = text.into();
            assert_eq!(preferences.target(), None, "{text}");
        }
    }

    #[test]
    fn target_input_accepts_persian_and_arabic_numeric_keyboards() {
        let mut preferences = Preferences::default();
        for text in ["۱٫۲۵", "١٫٢٥", "۱.25", " ٠٫٠٠١ "] {
            preferences.budget_draft = text.into();
            let expected = if text.contains('٠') {
                1_000
            } else {
                1_250_000
            };
            assert_eq!(preferences.target(), Some(expected), "{text}");
        }
        for text in ["۱٫۲۵ MB", "١٫٢٥text", "۱٫۲٫۵", "۰٫۰۰۰۹۹۹"] {
            preferences.budget_draft = text.into();
            assert_eq!(preferences.target(), None, "{text}");
        }
    }

    #[test]
    fn preferences_preserve_modes_and_sanitize_invalid_or_incompatible_settings() {
        let mut preferences = Preferences::default();
        assert!(preferences.settings_open);
        assert_eq!(preferences.settings.performance, "auto");
        assert_eq!(preferences.settings.concurrency, 0);
        preferences.settings.performance = "cpu".into();
        preferences.settings.concurrency = 5;
        preferences.settings_open = false;
        let encoded = serde_json::to_vec(&preferences).unwrap();
        let restored: Preferences = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(restored.settings.performance, "cpu");
        assert_eq!(restored.settings.concurrency, 5);
        assert!(!restored.settings_open);
        preferences.settings.performance = "invalid".into();
        preferences.settings.concurrency = 99;
        preferences.settings.format = "flac".into();
        preferences.settings.quality = "original".into();
        preferences.budget_enabled = true;
        preferences.sanitize();
        assert_eq!(preferences.settings.performance, "auto");
        assert_eq!(preferences.settings.concurrency, 0);
        assert_eq!(preferences.settings.format, "mp3");
        assert_eq!(preferences.settings.quality, "medium");
        assert_eq!(preferences.settings.target_bytes, Some(1_000_000));
    }

    #[test]
    fn slider_preserves_source_endpoint_and_minimum_without_enabling_empty_ranges() {
        assert_eq!(slider_bounds(Some(5_123_456)), (0.001, 5.123456, true));
        assert_eq!(slider_bounds(Some(1_000)), (0.001, 0.001, true));
        assert_eq!(slider_bounds(Some(500)), (0.0005, 0.0005, false));
        assert!(!slider_bounds(None).2);
        assert!(!slider_bounds(Some(0)).2);
        assert_eq!(decimal_mb(5.123456), "5.123456");
    }
}
