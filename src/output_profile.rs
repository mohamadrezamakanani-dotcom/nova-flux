//! Estimates based on configured rates or measured samples. Completed outputs have exact sizes.
use crate::engine::{Job, Settings};

pub const RESOLUTIONS: &[Option<u32>] = &[
    None,
    Some(2160),
    Some(1440),
    Some(1080),
    Some(720),
    Some(480),
    Some(360),
    Some(240),
    Some(144),
];

#[derive(Clone, Debug)]
pub struct Estimate {
    pub bytes: u64,
    pub lower_bytes: u64,
    pub upper_bytes: u64,
    pub method: &'static str,
    pub is_limit: bool,
}

/// Source raster dimensions are immutable. A legacy resize request has no
/// geometry result and must be rejected by the caller.
pub fn dimensions(width: u32, height: u32, resolution: Option<u32>) -> (u32, u32) {
    if width == 0 || height == 0 || resolution.is_some() {
        (0, 0)
    } else {
        (width, height)
    }
}

/// Maximum bytes for a lossy video preset, expressed as an intentional size
/// goal relative to the current input. This is not an output-size prediction.
/// Explicit targets, stream copy and audio profiles have no implicit ceiling.
pub fn preset_limit(job: &Job, settings: &Settings) -> Option<u64> {
    source_preset_limit(job.size, settings)
}

pub(crate) fn source_preset_limit(source_bytes: u64, settings: &Settings) -> Option<u64> {
    if settings.target_bytes.is_some()
        || !["mp4", "mov", "mkv", "avi", "webm"].contains(&settings.format.as_str())
    {
        return None;
    }
    let percent = match settings.quality.as_str() {
        "low" => 40,
        "medium" => 65,
        "high" => 85,
        _ => return None,
    };
    let limit = (source_bytes as u128 * percent / 100) as u64;
    (limit > 0 && limit < source_bytes).then_some(limit)
}

pub(crate) fn quality_index(quality: &str) -> usize {
    match quality {
        "low" => 0,
        "high" => 2,
        _ => 1,
    }
}

pub(crate) fn nominal_audio_bitrate(format: &str, quality: &str) -> u64 {
    let index = quality_index(quality);
    if format == "webm" {
        [64_000, 128_000, 192_000][index]
    } else {
        [64_000, 128_000, 320_000][index]
    }
}

pub(crate) fn hardware_video_bitrate(width: u32, height: u32, fps: f64, quality: &str) -> u64 {
    let index = quality_index(quality);
    let rate =
        (width as f64 * height as f64 * fps.clamp(1.0, 120.0) * [0.055, 0.11, 0.20][index]) as u64;
    rate.clamp([150_000, 400_000, 1_000_000][index], 100_000_000)
}

fn duration(selected: Option<f64>, fallback: f64) -> Option<f64> {
    let value = selected.unwrap_or(fallback);
    (value.is_finite() && value > 0.0).then_some(value)
}

fn result(bytes: f64, low: f64, high: f64, method: &'static str) -> Option<Estimate> {
    if !bytes.is_finite()
        || bytes <= 0.0
        || !low.is_finite()
        || !high.is_finite()
        || high > u64::MAX as f64
    {
        return None;
    }
    Some(Estimate {
        bytes: bytes.round() as u64,
        lower_bytes: low.floor().max(0.0) as u64,
        upper_bytes: high.ceil() as u64,
        method,
        is_limit: false,
    })
}

/// Estimate only profiles whose production arguments specify an audio bitrate,
/// PCM layout, or selected-stream copying. Content-dependent profiles require
/// real sample encodes through Engine::measure_estimate.
pub fn estimate(job: &Job, settings: &Settings) -> Option<Estimate> {
    let video = ["mp4", "mov", "mkv", "avi", "webm"].contains(&settings.format.as_str());
    if !video && !["mp3", "aac", "m4a", "ogg", "wav", "flac"].contains(&settings.format.as_str()) {
        return None;
    }
    if !["low", "medium", "high", "original"].contains(&settings.quality.as_str())
        || !RESOLUTIONS.contains(&settings.resolution)
        || !["cpu", "auto"].contains(&settings.performance.as_str())
        || video && settings.resolution.is_some()
        || !job.duration.is_finite()
        || job.duration <= 0.0
        || !crate::engine::profile_streams_compatible(job, settings)
    {
        return None;
    }
    let video_seconds = if video {
        duration(job.video_duration, job.duration)?
    } else {
        0.0
    };
    let audio_seconds = if job.audio.is_some() {
        // A video container can be much longer than its selected audio track.
        // Do not present that container length as an audio-extraction estimate.
        if !video && job.video.is_some() && job.audio_duration.is_none() {
            return None;
        }
        duration(job.audio_duration, job.duration)?
    } else {
        0.0
    };
    let (width, height) = if video {
        dimensions(job.width?, job.height?, settings.resolution)
    } else {
        (0, 0)
    };
    if video && (width == 0 || height == 0) {
        return None;
    }
    let fps = job.frame_rate.unwrap_or(30.0);
    if !fps.is_finite() || fps <= 0.0 {
        return None;
    }

    // A requested ceiling is a constraint, not a forecast of encoded bytes.
    if settings.target_bytes.is_some() {
        return None;
    }

    if settings.quality == "original" {
        let video_bytes = if video {
            job.video_bitrate? as f64 * video_seconds / 8.0
        } else {
            0.0
        };
        let audio_bytes = if job.audio.is_some() {
            job.audio_bitrate? as f64 * audio_seconds / 8.0
        } else {
            0.0
        };
        let payload = video_bytes + audio_bytes;
        return result(payload + 2_048.0, payload * 0.97, payload * 1.08 + 16_384.0,
            "Selected stream bitrates; remux container overhead and removed tracks affect the result.");
    }

    if settings.format == "flac" || settings.format == "ogg" || video {
        return None;
    }
    if settings.format == "wav" {
        let channels = job.channels.filter(|value| *value > 0)?;
        let sample_rate = job.sample_rate.filter(|value| *value > 0)?;
        let sample_format = job.sample_format.as_deref()?;
        if ![
            "u8", "u8p", "s16", "s16p", "s32", "s32p", "s64", "s64p", "flt", "fltp", "dbl", "dblp",
        ]
        .contains(&sample_format)
        {
            return None;
        }
        let bits = crate::engine::output_sample_bits(Some(sample_format), job.bits_per_sample);
        let payload = audio_seconds * sample_rate as f64 * channels as f64 * bits as f64 / 8.0;
        return result(payload + 80.0, payload + 44.0, payload + 8_192.0,
            "PCM sample rate × channels × output bit depth × duration; WAV header and copied metadata can vary.");
    }

    let audio_bytes =
        nominal_audio_bitrate(&settings.format, &settings.quality) as f64 * audio_seconds / 8.0;
    // This describes the configured payload budget. Container headers, metadata
    // and encoder packet padding remain unknown until measured.
    result(audio_bytes + 1_024.0, audio_bytes, audio_bytes + 1_024.0,
        "Configured audio bitrate × selected audio duration; header, metadata and encoder padding may change the final size.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_dimensions_keep_every_pixel_and_legacy_resize_has_no_result() {
        assert_eq!(dimensions(641, 361, None), (641, 361));
        assert_eq!(dimensions(3840, 2160, None), (3840, 2160));
        assert_eq!(dimensions(640, 360, Some(240)), (0, 0));
        assert_eq!(dimensions(0, 360, None), (0, 0));
    }

    #[test]
    fn video_preset_caps_are_distinct_source_relative_goals_and_never_claim_predictions() {
        let job = video_job();
        for (quality, expected) in [
            ("low", 4_000_000),
            ("medium", 6_500_000),
            ("high", 8_500_000),
        ] {
            assert_eq!(
                preset_limit(&job, &settings("mp4", quality)),
                Some(expected)
            );
        }
        assert_eq!(preset_limit(&job, &settings("mp4", "original")), None);
        assert_eq!(preset_limit(&job, &settings("wav", "high")), None);
        let mut explicit = settings("mp4", "low");
        explicit.target_bytes = Some(1_000_000);
        assert_eq!(preset_limit(&job, &explicit), None);
        let mut tiny = job.clone();
        tiny.size = 1;
        assert_eq!(preset_limit(&tiny, &settings("mp4", "low")), None);
        let mut huge = job;
        huge.size = u64::MAX;
        assert_eq!(
            preset_limit(&huge, &settings("mp4", "high")),
            Some(15_679_732_462_653_118_872)
        );
    }

    fn audio_job() -> Job {
        Job {
            id: "profile-test".into(),
            path: String::new(),
            name: "source.wav".into(),
            size: 10_000_000,
            source_revision: String::new(),
            duration: 10.0,
            video: None,
            audio: Some("pcm_s16le".into()),
            width: None,
            height: None,
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
            frame_rate: None,
            channels: Some(2),
            sample_rate: Some(48_000),
            sample_format: Some("s16".into()),
            bits_per_sample: Some(16),
            video_duration: None,
            audio_duration: Some(10.0),
            video_bitrate: None,
            audio_bitrate: Some(1_536_000),
        }
    }

    fn settings(format: &str, quality: &str) -> Settings {
        serde_json::from_value(serde_json::json!({
            "format":format,"quality":quality,"output_dir":"","concurrency":0,
            "performance":"cpu"
        }))
        .unwrap()
    }

    // Catches ignoring encoder bitrate or accidentally counting an unselected video stream.
    #[test]
    fn nominal_audio_estimate_follows_real_quality_bitrates() {
        let job = audio_job();
        let compact = estimate(&job, &settings("mp3", "low")).unwrap();
        let high = estimate(&job, &settings("mp3", "high")).unwrap();
        assert!((80_000..90_000).contains(&compact.bytes));
        assert!((400_000..415_000).contains(&high.bytes));
        assert!(!compact.is_limit);
        assert!(compact.lower_bytes <= compact.bytes && compact.upper_bytes >= compact.bytes);
    }

    #[test]
    fn invalid_duration_or_missing_selected_stream_has_no_estimate() {
        let mut job = audio_job();
        assert!(estimate(&job, &settings("mp4", "medium")).is_none());
        for duration in [0.0, f64::NAN, f64::INFINITY] {
            job.duration = duration;
            assert!(estimate(&job, &settings("mp3", "medium")).is_none());
        }
        job.duration = 10.0;
        assert!(estimate(&job, &settings("mp3", "original")).is_none());
    }

    // Catches a bitrate-style WAV estimate and treating floating-point PCM as 16-bit.
    #[test]
    fn pcm_estimate_counts_output_sample_bits_channels_and_selected_duration() {
        let mut job = audio_job();
        let pcm = estimate(&job, &settings("wav", "high")).unwrap();
        assert_eq!(pcm.bytes, 1_920_080);
        job.sample_format = Some("flt".into());
        let float = estimate(&job, &settings("wav", "low")).unwrap();
        assert_eq!(float.bytes, 3_840_080);
        job.sample_format = Some("s32".into());
        job.bits_per_sample = Some(24);
        job.channels = Some(1);
        job.audio_duration = Some(2.0);
        let mono = estimate(&job, &settings("wav", "medium")).unwrap();
        assert_eq!(mono.bytes, 288_080);
        job.sample_rate = None;
        assert!(estimate(&job, &settings("wav", "medium")).is_none());
    }

    #[test]
    fn variable_audio_requires_measured_content_instead_of_a_compression_guess() {
        let job = audio_job();
        assert!(estimate(&job, &settings("flac", "medium")).is_none());
        assert!(estimate(&job, &settings("ogg", "high")).is_none());
    }

    fn video_job() -> Job {
        let mut job = audio_job();
        job.video = Some("h264".into());
        job.audio = Some("aac".into());
        job.audio_bitrate = Some(128_000);
        job.width = Some(3840);
        job.height = Some(2160);
        job.frame_rate = Some(30.0);
        job.video_duration = Some(10.0);
        job.video_bitrate = Some(20_000_000);
        job
    }

    #[test]
    fn variable_video_requires_measured_content_instead_of_hardware_bitrate_guesses() {
        let job = video_job();
        for format in ["mp4", "mov", "mkv", "avi", "webm"] {
            for quality in ["low", "medium", "high"] {
                let mut config = settings(format, quality);
                assert!(estimate(&job, &config).is_none());
                config.resolution = Some(720);
                assert!(estimate(&job, &config).is_none());
            }
        }
    }

    #[test]
    fn audio_extraction_counts_selected_track_and_copy_does_not_count_video_bytes() {
        let mut job = video_job();
        job.audio_duration = Some(2.0);
        let extracted = estimate(&job, &settings("mp3", "medium")).unwrap();
        assert_eq!(extracted.bytes, 33_024);
        let copied = estimate(&job, &settings("m4a", "original")).unwrap();
        assert_eq!(copied.bytes, 34_048);
        job.audio_duration = None;
        assert!(estimate(&job, &settings("mp3", "medium")).is_none());
    }

    #[test]
    fn a_requested_limit_is_not_an_estimate_of_encoded_bytes() {
        let job = audio_job();
        let mut config = settings("mp3", "medium");
        config.target_bytes = Some(500_000);
        assert!(estimate(&job, &config).is_none());
    }
}
