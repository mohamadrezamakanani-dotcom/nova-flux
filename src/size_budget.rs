//! Codec-aware size planning. Byte limits are enforced by measured output in the engine.
#[derive(Clone, Debug)]
pub(crate) struct Input {
    pub duration: f64,
    pub video: bool,
    pub audio: bool,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub channels: u32,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Plan {
    pub video_bitrate: Option<u64>,
    pub audio_bitrate: Option<u64>,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub sample_rate: u32,
    pub channels: u32,
}
pub(crate) fn calculate(
    input: &Input,
    format: &str,
    target: u64,
    adjustment: f64,
) -> Result<Plan, String> {
    let video = ["mp4", "mov", "mkv", "avi", "webm"].contains(&format);
    if !video && !["mp3", "aac", "m4a", "ogg"].contains(&format) {
        return Err("Target-size compression supports lossy formats only. Choose MP4, MOV, MKV, AVI, WebM, MP3, AAC, M4A or OGG.".into());
    }
    if !input.duration.is_finite() || input.duration <= 0.0 {
        return Err("Target-size compression requires a known, positive media duration.".into());
    }
    if video && !input.video || !video && !input.audio {
        return Err(
            "The selected output needs a media stream that is missing from this file.".into(),
        );
    }
    if video && (input.width == 0 || input.height == 0) {
        return Err("Preserving video dimensions requires readable source geometry.".into());
    }
    if video
        && format == "avi"
        && (!input.width.is_multiple_of(2) || !input.height.is_multiple_of(2))
    {
        return Err("AVI encoding cannot preserve odd video dimensions. Choose MP4, MOV, MKV or WebM; the source will not be resized.".into());
    }
    let audio_codec = match format {
        "mp3" | "avi" => "mp3",
        "webm" => "opus",
        "ogg" => "vorbis",
        _ => "aac",
    };
    let minimum_audio = match audio_codec {
        "opus" => 6_000,
        "vorbis" => 16_000,
        _ => 8_000,
    };
    let minimum_video = 6_000;
    let static_overhead = match format {
        "avi" => 8_192.0,
        "mp4" | "mov" | "m4a" => 1_500.0,
        "ogg" => 4_000.0,
        _ => 700.0,
    };
    let minimum = static_overhead
        + input.duration
            * (if video { minimum_video } else { 0 } + if input.audio { minimum_audio } else { 0 })
                as f64
            / 8.0;
    if (target as f64) < minimum * 1.04 {
        return Err(format!("The requested size ({target} bytes) is too small to preserve the complete {:.1}-second media. Try at least {} bytes or a shorter source; no file was created.", input.duration, (minimum * 1.12).ceil() as u64));
    }
    // Two refinements account for packet/index overhead after frame/sample-rate selection.
    let mut overhead = static_overhead + target as f64 * 0.015;
    let mut plan = Plan {
        video_bitrate: None,
        audio_bitrate: None,
        width: input.width,
        height: input.height,
        frame_rate: input.frame_rate,
        sample_rate: 48_000,
        channels: input.channels.clamp(1, 2),
    };
    for _ in 0..2 {
        let total_rate =
            ((target as f64 - overhead).max(0.0) * 8.0 / input.duration * 0.985 * adjustment)
                .floor() as u64;
        if total_rate
            < (if video { minimum_video } else { 0 })
                + (if input.audio { minimum_audio } else { 0 })
        {
            return Err("This size is below the minimum usable bitrate after container overhead. Increase the target size.".into());
        }
        let audio_allowance = if !input.audio {
            0
        } else if video {
            ((total_rate as f64 * 0.18) as u64).clamp(minimum_audio, 128_000)
        } else {
            total_rate.min(320_000)
        };
        let audio_rate = match audio_codec {
            "mp3" => [
                8_000, 16_000, 24_000, 32_000, 40_000, 48_000, 56_000, 64_000, 80_000, 96_000,
                112_000, 128_000, 160_000, 192_000, 224_000, 256_000, 320_000,
            ]
            .into_iter()
            .filter(|rate| *rate <= audio_allowance)
            .max()
            .unwrap_or(0),
            "opus" => (audio_allowance / 1000 * 1000).min(192_000),
            _ => audio_allowance / 1000 * 1000,
        };
        if input.audio && audio_rate < minimum_audio {
            return Err("This target cannot fit a complete audio stream at a supported bitrate. Increase the size.".into());
        }
        plan.audio_bitrate = input.audio.then_some(audio_rate);
        plan.channels = if audio_rate < 64_000 {
            1
        } else {
            input.channels.clamp(1, 2)
        };
        plan.sample_rate = match audio_codec {
            "mp3" => {
                if audio_rate <= 16_000 {
                    8_000
                } else if audio_rate <= 32_000 {
                    16_000
                } else if audio_rate <= 56_000 {
                    24_000
                } else {
                    44_100
                }
            }
            "opus" => {
                if audio_rate <= 12_000 {
                    8_000
                } else if audio_rate <= 24_000 {
                    16_000
                } else if audio_rate <= 40_000 {
                    24_000
                } else {
                    48_000
                }
            }
            _ => {
                if audio_rate <= 16_000 {
                    8_000
                } else if audio_rate <= 24_000 {
                    12_000
                } else if audio_rate <= 40_000 {
                    16_000
                } else if audio_rate <= 64_000 {
                    24_000
                } else {
                    48_000
                }
            }
        };
        if video {
            let video_rate = total_rate.saturating_sub(audio_rate) / 1000 * 1000;
            if video_rate < minimum_video {
                return Err(
                    "This target cannot fit a complete video stream. Increase the size.".into(),
                );
            }
            plan.video_bitrate = Some(video_rate);
            // Byte budgets may change bitrate, never picture geometry or cadence.
        }
        let video_packets = if video { plan.frame_rate } else { 0.0 };
        let audio_packets = if !input.audio {
            0.0
        } else if audio_codec == "opus" {
            50.0
        } else if audio_codec == "mp3" {
            plan.sample_rate as f64
                / (if plan.sample_rate < 32_000 {
                    576.0
                } else {
                    1152.0
                })
        } else {
            plan.sample_rate as f64 / 1024.0
        };
        let packet_bytes = match format {
            "avi" => 24.0,
            "mp4" | "mov" | "m4a" => 12.0,
            "aac" => 7.0,
            "mp3" => 0.0,
            _ => 10.0,
        };
        overhead = static_overhead
            + (video_packets + audio_packets) * packet_bytes * input.duration
            + target as f64 * 0.015;
    }
    Ok(plan)
}

impl Plan {
    pub(crate) fn arguments(&self, format: &str, first_pass: bool) -> Vec<String> {
        let mut args = Vec::new();
        let mut add = |values: &[&str]| args.extend(values.iter().map(|s| s.to_string()));
        let video = self.video_bitrate.is_some();
        if video {
            add(&["-map", "0:V:0"]);
        }
        if first_pass {
            add(&["-an"]);
        } else if self.audio_bitrate.is_some() {
            add(&["-map", "0:a:0"]);
        }
        if !video {
            add(&["-vn"]);
        }
        add(&["-sn", "-dn", "-map_metadata", "-1", "-map_chapters", "-1"]);
        if let Some(rate) = self.video_bitrate {
            add(&[
                "-pix_fmt",
                if self.width.is_multiple_of(2) && self.height.is_multiple_of(2) {
                    "yuv420p"
                } else {
                    "yuv444p"
                },
                "-fps_mode",
                "passthrough",
                "-enc_time_base:v",
                "-1",
            ]);
            match format {
                "webm" => add(&[
                    "-c:v",
                    "libvpx-vp9",
                    "-deadline",
                    "good",
                    "-cpu-used",
                    "2",
                    "-row-mt",
                    "1",
                ]),
                "avi" => add(&["-c:v", "mpeg4"]),
                _ => add(&["-c:v", "libx264", "-preset", "medium"]),
            }
            add(&["-b:v", &rate.to_string()]);
        }
        if !first_pass {
            if let Some(rate) = self.audio_bitrate {
                let codec = match format {
                    "mp3" | "avi" => "libmp3lame",
                    "ogg" => "libvorbis",
                    "webm" => "libopus",
                    _ => "aac",
                };
                add(&[
                    "-c:a",
                    codec,
                    "-b:a",
                    &rate.to_string(),
                    "-ar",
                    &self.sample_rate.to_string(),
                    "-ac",
                    &self.channels.to_string(),
                ]);
                if codec == "libopus" {
                    add(&["-vbr", "off"]);
                }
            }
            if ["mp4", "mov", "m4a"].contains(&format) {
                add(&["-movflags", "+faststart"]);
            }
            let muxer = match format {
                "mkv" => "matroska",
                "m4a" => "ipod",
                "aac" => "adts",
                other => other,
            };
            add(&["-f", muxer]);
        } else {
            add(&["-f", "null"]);
        }
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn video() -> Input {
        Input {
            duration: 10.0,
            video: true,
            audio: true,
            width: 640,
            height: 360,
            frame_rate: 24.0,
            channels: 2,
        }
    }
    #[test]
    fn smaller_video_budget_changes_quality_without_changing_geometry_or_cadence() {
        let large = calculate(&video(), "mp4", 1_000_000, 1.0).unwrap();
        let small = calculate(&video(), "mp4", 100_000, 1.0).unwrap();
        assert!(small.video_bitrate.unwrap() < large.video_bitrate.unwrap());
        assert_eq!(
            (large.width, large.height, large.frame_rate),
            (640, 360, 24.0)
        );
        assert_eq!(
            (small.width, small.height, small.frame_rate),
            (640, 360, 24.0)
        );
        assert!(small.audio_bitrate.unwrap() >= 8_000);
        let arguments = small.arguments("mp4", false);
        assert!(!arguments
            .iter()
            .any(|argument| ["-vf", "-r"].contains(&argument.as_str())));
    }
    #[test]
    fn odd_and_high_rate_video_keep_every_source_pixel_and_frame() {
        let input = Input {
            width: 641,
            height: 361,
            frame_rate: 59.94,
            ..video()
        };
        let plan = calculate(&input, "mp4", 1_000_000, 1.0).unwrap();
        assert_eq!(
            (plan.width, plan.height, plan.frame_rate),
            (641, 361, 59.94)
        );
        assert!(plan
            .arguments("mp4", false)
            .windows(2)
            .any(|pair| pair == ["-pix_fmt", "yuv444p"]));
        assert!(calculate(&input, "avi", 1_000_000, 1.0).is_err());
    }
    #[test]
    fn audio_budget_chooses_feasible_sample_rate_and_bitrate() {
        let input = Input {
            video: false,
            ..video()
        };
        let plan = calculate(&input, "mp3", 50_000, 1.0).unwrap();
        assert!(plan.video_bitrate.is_none());
        assert!(plan.audio_bitrate.unwrap() <= 40_000);
        assert!(plan.sample_rate <= 24_000);
        assert_eq!(plan.channels, 1);
    }
    #[test]
    fn impossible_unknown_or_lossless_budget_is_rejected() {
        assert!(calculate(&video(), "mp4", 1_000, 1.0).is_err());
        assert!(calculate(
            &Input {
                duration: 0.0,
                ..video()
            },
            "mp4",
            1_000_000,
            1.0
        )
        .is_err());
        assert!(calculate(&video(), "wav", 1_000_000, 1.0).is_err());
        assert!(calculate(&video(), "flac", 1_000_000, 1.0).is_err());
    }
}
