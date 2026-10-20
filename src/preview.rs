//! Short real encodes for profiles whose quality setting cannot predict bitrate.
use super::*;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct MeasuredEstimate {
    pub estimate: crate::output_profile::Estimate,
    pub encoder: String,
    pub sampled_seconds: f64,
    pub samples: usize,
}

impl Engine {
    /// Measure the production profile on local samples. The observed spread is
    /// not a confidence interval or a guarantee for the unsampled content.
    pub fn measure_estimate(
        &self,
        job: &Job,
        settings: &Settings,
        cancel: &AtomicBool,
    ) -> Result<MeasuredEstimate, String> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let check = || preview_check(&self.shared.closed, cancel, deadline);
        check()?;
        self.ensure_open()?;
        self.ensure_runtime()?;
        if settings.target_bytes.is_some() {
            return Err("A size limit is a maximum, not an estimate. Disable the limit to measure a quality profile.".into());
        }
        if self.shared.lock().running {
            return Err("Pause size measurements while conversion is running.".into());
        }
        let input = PathBuf::from(&job.path);
        if std::fs::canonicalize(&input).map_err(|e| format!("Input file is unavailable: {e}"))?
            != input
        {
            return Err("Input path changed. Remove this file and import it again.".into());
        }
        let (size, revision) = source_identity(&input)?;
        let media = probe_before(
            &self.runtime,
            &input,
            &self.shared.closed,
            Some(cancel),
            deadline,
        )?;
        check()?;
        if source_identity(&input)?.1 != revision {
            return Err(
                "Input changed during inspection. Wait for it to finish changing, then retry."
                    .into(),
            );
        }
        if revision != job.source_revision {
            // Publish current metadata, never a sample based on a stale UI job.
            // The desktop invalidates its cache on the resulting revision.
            if let Some(current) = self.shared.lock().jobs.iter_mut().find(|j| j.id == job.id) {
                apply_media(current, &media);
                current.size = size;
                current.source_revision = revision;
            }
            return Err("Input changed since import. Its metadata has been refreshed; measure the updated source.".into());
        }
        let (compression_settings, _) = compression_profile(settings, size)?;
        let settings = &compression_settings;
        let workspace = tempfile::Builder::new()
            .prefix(".nova-estimate-")
            .tempdir()
            .map_err(|e| format!("Cannot create a private measurement workspace: {e}"))?;
        let mut profile = settings.clone();
        // Measuring never needs a user save folder or publishes a result there.
        profile.output_dir = workspace.path().to_string_lossy().into_owned();
        validate_settings(&profile)?;
        validate_streams(&media, &settings.format, &settings.quality)?;
        validate_video_geometry(&media, settings)?;
        let total_seconds = progress_duration(&media, &settings.format);
        if !total_seconds.is_finite() || total_seconds <= 0.0 {
            return Err("A sample estimate needs a known duration for the selected streams. Size will be measured after conversion.".into());
        }
        let video = VIDEO_FORMATS.contains(&settings.format.as_str());
        if video && (media.width.unwrap_or(0) < 2 || media.height.unwrap_or(0) < 2) {
            return Err("A video size measurement needs readable source dimensions. Try importing the file again.".into());
        }
        let automatic = cfg!(target_os = "macos")
            && settings.performance == "auto"
            && settings.quality != "original"
            && ["mp4", "mov", "mkv"].contains(&settings.format.as_str())
            && even_geometry(&media);
        let (queued, pixels) = {
            let state = self.shared.lock();
            (
                state
                    .jobs
                    .iter()
                    .filter(|j| j.status == "queued")
                    .count()
                    .max(1),
                state
                    .jobs
                    .iter()
                    .filter(|j| j.status == "queued")
                    .map(|j| j.width.unwrap_or(0) as u64 * j.height.unwrap_or(0) as u64)
                    .max()
                    .unwrap_or(0),
            )
        };
        let schedule = crate::performance::schedule(
            crate::performance::resources(),
            queued,
            settings.concurrency,
            video,
            automatic,
            pixels.max(media.width.unwrap_or(0) as u64 * media.height.unwrap_or(0) as u64),
        );
        let threads = schedule.threads_per_worker;
        let mut hardware = false;
        if automatic {
            let workload =
                calibration_workload(&input, &media, settings, threads).map_err(|failure| {
                    match failure {
                        Failure::Cancelled => "Cancelled".into(),
                        Failure::Error(error) => error,
                    }
                })?;
            let selection = self.calibration.select_before(
                &self.runtime,
                &workload,
                workspace.path(),
                &self.shared.closed,
                cancel,
                deadline,
            );
            check()?;
            // A calibration timeout still leaves the bounded CPU path available.
            hardware = selection
                .map(|selection| selection.hardware)
                .unwrap_or(false);
        }
        // Seeking a copied video can retain packets from the previous keyframe.
        // Their bytes must never be extrapolated over a nominal short interval.
        // Copy profiles are cheap remuxes, so measure one complete production
        // remux instead, with the same bounded deadline and cancellation.
        let whole_copy = settings.quality == "original";
        let offsets = if whole_copy {
            vec![(0.0, total_seconds)]
        } else {
            sample_windows(total_seconds)
        };
        let mut observations = Vec::new();
        let mut actual_seconds = 0.0;
        let mut encoder = String::new();
        let sample_encoder = SampleEncoder {
            runtime: &self.runtime,
            input: &input,
            threads,
            closed: &self.shared.closed,
            cancel,
            deadline,
            source_revision: &revision,
        };
        for (index, (offset, seconds)) in offsets.iter().copied().enumerate() {
            check()?;
            let output = workspace
                .path()
                .join(format!("sample-{index}.{}", settings.format));
            let mut arguments = measurement_arguments(&media, settings, hardware, size)?;
            let pass_log = workspace.path().join(format!("pass-{index}"));
            let mut result = sample_encoder.encode_profile(
                &output,
                &arguments,
                offset,
                (!whole_copy).then_some(seconds),
                &pass_log,
            );
            self.ensure_sample_source(job, &input, &revision, cancel, deadline)?;
            if result.is_err() && hardware {
                check()?;
                // Discard earlier hardware samples; mixing encoders would not
                // measure any single profile that production can actually use.
                arguments = measurement_arguments(&media, settings, false, size)?;
                hardware = false;
                if index != 0 {
                    return Err("Apple hardware stopped during measurement. Retry using CPU encoding for a consistent sample estimate.".into());
                }
                result = sample_encoder.encode_profile(
                    &output,
                    &arguments,
                    offset,
                    (!whole_copy).then_some(seconds),
                    &pass_log,
                );
                self.ensure_sample_source(job, &input, &revision, cancel, deadline)?;
            }
            result?;
            check()?;
            let sample = probe_before(
                &self.runtime,
                &output,
                &self.shared.closed,
                Some(cancel),
                deadline,
            )?;
            let duration = progress_duration(&sample, &settings.format);
            if !duration.is_finite() || duration <= 0.0 || duration < seconds * 0.75 {
                return Err("The encoded sample has no reliable complete duration. Final size will be measured after conversion.".into());
            }
            validate_streams(&sample, &settings.format, "medium")?;
            if whole_copy {
                // A complete-copy measurement must pass the same completeness
                // checks as an output the production engine would publish.
                verify_video_geometry(&media, &sample, settings)?;
                verify_stream_duration(media.audio_duration, sample.audio_duration, "audio")?;
                if video {
                    verify_stream_duration(media.video_duration, sample.video_duration, "video")?;
                }
            } else if video {
                verify_video_dimensions(&media, &sample)?;
            }
            if media.audio.is_some() && sample.audio.is_none() {
                return Err("The sampled interval has no audio from the selected stream. Final size will be measured after conversion.".into());
            }
            let bytes = std::fs::metadata(&output)
                .map_err(|e| format!("Cannot measure sample: {e}"))?
                .len();
            if bytes == 0 {
                return Err(
                    "The encoder produced an empty measurement sample. Try another format.".into(),
                );
            }
            let encoder_arguments = arguments
                .last()
                .ok_or("No production encoder is available for this sample.")?;
            encoder = encoder_arguments
                .windows(2)
                .find(|pair| pair[0] == "-c:v")
                .or_else(|| encoder_arguments.windows(2).find(|pair| pair[0] == "-c:a"))
                .map(|pair| pair[1].clone())
                .unwrap_or_else(|| "unknown".into());
            if encoder == "copy" {
                encoder = "stream copy".into();
            }
            actual_seconds += duration;
            observations.push((bytes, duration));
            // Do not retain encoded media while the next interval is measured.
            std::fs::remove_file(output)
                .map_err(|e| format!("Cannot remove private sample: {e}"))?;
        }
        check()?;
        self.ensure_sample_source(job, &input, &revision, cancel, deadline)?;
        let mut estimate = extrapolate(&observations, total_seconds, offsets.len() == 1)?;
        if whole_copy {
            estimate.method = "Measured complete stream-copy remux with production mappings; no seeking, duration trim or sample extrapolation. Final bytes are measured again when saving.";
        } else if settings.target_bytes.is_some() {
            estimate.method = "Real sample encodes of the initial source-relative bitrate plan. Production may refine bitrate using measured full-output sizes while enforcing the preset maximum; this sample evidence is not a prediction of the final refined file.";
        }
        Ok(MeasuredEstimate {
            estimate,
            encoder,
            sampled_seconds: actual_seconds,
            samples: observations.len(),
        })
    }

    fn ensure_sample_source(
        &self,
        job: &Job,
        input: &Path,
        expected: &str,
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> Result<(), String> {
        preview_check(&self.shared.closed, cancel, deadline)?;
        let (fresh_size, fresh_revision) = source_identity(input)?;
        if fresh_revision == expected {
            return Ok(());
        }
        let fresh_media = probe_before(
            &self.runtime,
            input,
            &self.shared.closed,
            Some(cancel),
            deadline,
        );
        let stable = source_identity(input).is_ok_and(|identity| identity.1 == fresh_revision);
        if let Some(current) = self.shared.lock().jobs.iter_mut().find(|j| j.id == job.id) {
            if let (Ok(fresh_media), true) = (fresh_media, stable) {
                apply_media(current, &fresh_media);
                current.size = fresh_size;
                current.source_revision = fresh_revision;
            } else {
                current.status = "error".into();
                current.error = Some("Input changed during sample encoding. Retry after the source finishes changing to refresh its metadata.".into());
            }
        }
        Err("Input changed during sample encoding. Discarded the estimate; retry after the source finishes changing.".into())
    }
}

fn preview_check(
    closed: &AtomicBool,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<(), String> {
    if closed.load(Ordering::Acquire) || cancel.load(Ordering::Acquire) {
        Err("Cancelled".into())
    } else if Instant::now() >= deadline {
        Err("Sample measurement exceeded 15 seconds. Try a faster encoding mode or convert to measure the exact size.".into())
    } else {
        Ok(())
    }
}

fn sample_windows(seconds: f64) -> Vec<(f64, f64)> {
    if seconds <= 3.0 {
        vec![(0.0, seconds)]
    } else {
        let length = 1.25_f64.min(seconds / 3.0);
        [0.10, 0.50, 0.90]
            .into_iter()
            .map(|fraction| {
                (
                    (seconds * fraction - length / 2.0).clamp(0.0, seconds - length),
                    length,
                )
            })
            .collect()
    }
}

fn extrapolate(
    samples: &[(u64, f64)],
    duration: f64,
    whole: bool,
) -> Result<crate::output_profile::Estimate, String> {
    if samples.is_empty() || !duration.is_finite() || duration <= 0.0 {
        return Err("No usable measurement samples are available.".into());
    }
    let (bytes, low, high) = if whole {
        let bytes = samples[0].0 as f64;
        (bytes, bytes, bytes)
    } else {
        let total_bytes: f64 = samples.iter().map(|sample| sample.0 as f64).sum();
        let total_seconds: f64 = samples.iter().map(|sample| sample.1).sum();
        let rates: Vec<f64> = samples
            .iter()
            .map(|sample| sample.0 as f64 / sample.1)
            .collect();
        (
            total_bytes / total_seconds * duration,
            rates.iter().copied().reduce(f64::min).unwrap() * duration,
            rates.iter().copied().reduce(f64::max).unwrap() * duration,
        )
    };
    if !bytes.is_finite()
        || !low.is_finite()
        || !high.is_finite()
        || high > u64::MAX as f64
        || low <= 0.0
    {
        return Err("Measured sample size cannot be safely extrapolated.".into());
    }
    Ok(crate::output_profile::Estimate {
        bytes: bytes.round() as u64,
        lower_bytes: low.floor() as u64,
        upper_bytes: high.ceil() as u64,
        is_limit: false,
        method: if whole {
            "Measured complete short sample with production encoder settings; final encoder choice and metadata can still change the result."
        } else {
            "Measured sample bytes divided by their measured durations, extrapolated to the selected timeline. Observed sample spread is not a confidence interval; unsampled content, keyframes and repeated sample headers can change the final size."
        },
    })
}

fn measurement_arguments(
    media: &Media,
    settings: &Settings,
    hardware: bool,
    source_bytes: u64,
) -> Result<Vec<Vec<String>>, String> {
    if let Some(target) = settings.target_bytes {
        let plan = crate::size_budget::calculate(
            &planning_input(media, settings),
            &settings.format,
            target.min(source_bytes.saturating_sub(1)),
            1.0,
        )?;
        return Ok(if hardware {
            vec![use_hardware_encoder(
                plan.arguments(&settings.format, false),
                None,
            )]
        } else {
            vec![
                plan.arguments(&settings.format, true),
                plan.arguments(&settings.format, false),
            ]
        });
    }
    Ok(vec![transcode_arguments(media, settings, hardware)])
}

struct SampleEncoder<'a> {
    runtime: &'a Path,
    input: &'a Path,
    threads: usize,
    closed: &'a AtomicBool,
    cancel: &'a AtomicBool,
    deadline: Instant,
    source_revision: &'a str,
}

impl SampleEncoder<'_> {
    fn encode_profile(
        &self,
        output: &Path,
        passes: &[Vec<String>],
        offset: f64,
        seconds: Option<f64>,
        pass_log: &Path,
    ) -> Result<(), String> {
        for (index, pass) in passes.iter().enumerate() {
            let mut arguments = pass.clone();
            if passes.len() > 1 {
                arguments.extend([
                    "-pass".into(),
                    (index + 1).to_string(),
                    "-passlogfile".into(),
                    pass_log.to_string_lossy().into_owned(),
                ]);
            }
            let destination = if index + 1 == passes.len() {
                output
            } else {
                Path::new("-")
            };
            self.encode(destination, &arguments, offset, seconds)?;
        }
        Ok(())
    }

    fn encode(
        &self,
        output: &Path,
        arguments: &[String],
        offset: f64,
        seconds: Option<f64>,
    ) -> Result<(), String> {
        let (runtime, input, threads, closed, cancel, deadline) = (
            self.runtime,
            self.input,
            self.threads,
            self.closed,
            self.cancel,
            self.deadline,
        );
        preview_check(closed, cancel, deadline)?;
        if source_identity(input)?.1 != self.source_revision {
            return Err("Input changed during sample encoding. Discarded the estimate.".into());
        }
        let threads = threads.max(1).to_string();
        let mut command = crate::platform::command(crate::platform::executable(runtime, "ffmpeg"));
        command.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "file,pipe",
            "-threads",
            &threads,
        ]);
        if seconds.is_some() {
            command.args(["-ss", &format!("{offset:.6}")]);
        }
        command.arg("-i").arg(input);
        if let Some(seconds) = seconds {
            command.args(["-t", &format!("{seconds:.6}")]);
        }
        command
            .args([
                "-threads",
                &threads,
                "-filter_threads",
                &threads,
                "-filter_complex_threads",
                &threads,
            ])
            .args(arguments)
            .args(["-y"])
            .arg(output)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|e| format!("Cannot start sample encoder: {e}"))?;
        let stderr = child.stderr.take().expect("piped encoder stderr");
        let reader = std::thread::spawn(move || read_bounded(stderr, 32 * 1024));
        let status = loop {
            if let Err(error) = preview_check(closed, cancel, deadline) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(error);
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err(format!("Sample encoder failed: {error}"));
                }
            }
        };
        let (errors, _) = reader.join().unwrap_or_default();
        preview_check(closed, cancel, deadline)?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "Cannot encode this measurement sample: {}",
                concise_error(&errors)
            ))
        }
    }
}
