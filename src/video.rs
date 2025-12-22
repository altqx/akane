use crate::types::{
    AttachmentInfo, AudioStreamInfo, ChapterInfo, ProgressMap, ProgressUpdate, SubtitleStreamInfo,
    VideoVariant,
};
use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
use tokio::time::{Duration, sleep};
use tokio::{fs, process::Command};
use tracing::{error, info, warn};

pub async fn get_video_metadata(input: &PathBuf) -> Result<(u32, u32)> {
    // Using JSON output
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=height:format=duration")
        .arg("-of")
        .arg("json")
        .arg(input)
        .output()
        .await
        .context("failed to run ffprobe")?;

    if !output.status.success() {
        anyhow::bail!("ffprobe failed");
    }

    let json_str = String::from_utf8(output.stdout)?;
    let v: serde_json::Value = serde_json::from_str(&json_str)?;

    let height = v["streams"][0]["height"]
        .as_u64()
        .context("no height found")? as u32;
    let duration_str = v["format"]["duration"]
        .as_str()
        .context("no duration found")?;
    let duration: f64 = duration_str.parse()?;

    Ok((height, duration.round() as u32))
}

pub async fn get_video_height(input: &PathBuf) -> Result<u32> {
    // Keep for backward compatibility or individual usage
    let (h, _) = get_video_metadata(input).await?;
    Ok(h)
}

pub async fn get_video_duration(input: &PathBuf) -> Result<u32> {
    // Keep for backward compatibility or individual usage
    let (_, d) = get_video_metadata(input).await?;
    Ok(d)
}

// Get audio stream information from video file using ffprobe
pub async fn get_audio_streams(input: &PathBuf) -> Result<Vec<AudioStreamInfo>> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("a")
        .arg("-show_entries")
        .arg("stream=index,codec_name,channels,sample_rate,bit_rate:stream_tags=language,title:stream_disposition=default")
        .arg("-of")
        .arg("json")
        .arg(input)
        .output()
        .await
        .context("failed to run ffprobe for audio streams")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffprobe for audio streams failed: {stderr}");
    }

    let json_str = String::from_utf8(output.stdout)?;
    let v: serde_json::Value = serde_json::from_str(&json_str)?;

    let streams = v["streams"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .enumerate()
                .map(|(idx, s)| AudioStreamInfo {
                    stream_index: s["index"].as_i64().unwrap_or(idx as i64) as i32,
                    codec_name: s["codec_name"].as_str().unwrap_or("unknown").to_string(),
                    language: s["tags"]["language"].as_str().map(|s| s.to_string()),
                    title: s["tags"]["title"].as_str().map(|s| s.to_string()),
                    channels: s["channels"].as_i64().map(|c| c as i32),
                    sample_rate: s["sample_rate"]
                        .as_str()
                        .and_then(|sr| sr.parse::<i32>().ok()),
                    bit_rate: s["bit_rate"].as_str().and_then(|br| br.parse::<i64>().ok()),
                    is_default: s["disposition"]["default"].as_i64().unwrap_or(0) == 1,
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(streams)
}

// Get subtitle stream information from video file using ffprobe
pub async fn get_subtitle_streams(input: &PathBuf) -> Result<Vec<SubtitleStreamInfo>> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("s")
        .arg("-show_entries")
        .arg("stream=index,codec_name:stream_tags=language,title:stream_disposition=default,forced")
        .arg("-of")
        .arg("json")
        .arg(input)
        .output()
        .await
        .context("failed to run ffprobe for subtitles")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffprobe for subtitles failed: {stderr}");
    }

    let json_str = String::from_utf8(output.stdout)?;
    let v: serde_json::Value = serde_json::from_str(&json_str)?;

    let streams = v["streams"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .enumerate()
                .map(|(idx, s)| SubtitleStreamInfo {
                    stream_index: s["index"].as_i64().unwrap_or(idx as i64) as i32,
                    codec_name: s["codec_name"].as_str().unwrap_or("unknown").to_string(),
                    language: s["tags"]["language"].as_str().map(|s| s.to_string()),
                    title: s["tags"]["title"].as_str().map(|s| s.to_string()),
                    is_default: s["disposition"]["default"].as_i64().unwrap_or(0) == 1,
                    is_forced: s["disposition"]["forced"].as_i64().unwrap_or(0) == 1,
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(streams)
}

// Get attachment information (fonts) from video file using ffprobe
pub async fn get_attachments(input: &PathBuf) -> Result<Vec<AttachmentInfo>> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("t")
        .arg("-show_entries")
        .arg("stream=index:stream_tags=filename,mimetype")
        .arg("-of")
        .arg("json")
        .arg(input)
        .output()
        .await
        .context("failed to run ffprobe for attachments")?;

    if !output.status.success() {
        // No attachments is not an error
        return Ok(Vec::new());
    }

    let json_str = String::from_utf8(output.stdout)?;
    let v: serde_json::Value = serde_json::from_str(&json_str)?;

    let attachments = v["streams"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|s| {
                    let filename = s["tags"]["filename"].as_str()?;
                    let mimetype = s["tags"]["mimetype"].as_str().unwrap_or_else(|| {
                        // Guess mimetype from extension
                        let lowercase = filename.to_lowercase();
                        if lowercase.ends_with(".ttf") {
                            "font/ttf"
                        } else if lowercase.ends_with(".otf") {
                            "font/otf"
                        } else if lowercase.ends_with(".ttc") {
                            "font/collection"
                        } else if lowercase.ends_with(".woff") {
                            "font/woff"
                        } else if lowercase.ends_with(".woff2") {
                            "font/woff2"
                        } else {
                            "application/octet-stream"
                        }
                    });
                    Some(AttachmentInfo {
                        filename: filename.to_string(),
                        mimetype: mimetype.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(attachments)
}

// Get chapter information from video file using ffprobe
pub async fn get_chapters(input: &PathBuf) -> Result<Vec<ChapterInfo>> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_chapters")
        .arg("-of")
        .arg("json")
        .arg(input)
        .output()
        .await
        .context("failed to run ffprobe for chapters")?;

    if !output.status.success() {
        // No chapters is not an error
        return Ok(Vec::new());
    }

    let json_str = String::from_utf8(output.stdout)?;
    let v: serde_json::Value = serde_json::from_str(&json_str)?;

    let chapters = v["chapters"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    let start_time = c["start_time"]
                        .as_str()
                        .and_then(|s| s.parse::<f64>().ok())
                        .or_else(|| c["start_time"].as_f64())?;
                    let end_time = c["end_time"]
                        .as_str()
                        .and_then(|s| s.parse::<f64>().ok())
                        .or_else(|| c["end_time"].as_f64())?;
                    let title = c["tags"]["title"].as_str().unwrap_or("").to_string();
                    Some(ChapterInfo {
                        start_time,
                        end_time,
                        title,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(chapters)
}

/// Check if a subtitle codec is a bitmap-based format (PGS, VobSub, DVB)
pub fn is_bitmap_subtitle(codec: &str) -> bool {
    matches!(
        codec.to_lowercase().as_str(),
        "hdmv_pgs_subtitle" | "pgssub" | "dvd_subtitle" | "dvdsub" | "dvb_subtitle" | "dvbsub"
    )
}

/// Check if a subtitle codec is VobSub (DVD subtitle format)
pub fn is_vobsub_subtitle(codec: &str) -> bool {
    matches!(codec.to_lowercase().as_str(), "dvd_subtitle" | "dvdsub")
}

/// Get the file extension for a subtitle codec
#[allow(dead_code)]
pub fn get_subtitle_extension(codec: &str) -> &'static str {
    if is_bitmap_subtitle(codec) {
        match codec.to_lowercase().as_str() {
            "hdmv_pgs_subtitle" | "pgssub" => "sup",
            _ => "sub",
        }
    } else {
        match codec.to_lowercase().as_str() {
            "ass" | "ssa" => "ass",
            "subrip" | "srt" => "srt",
            "webvtt" | "vtt" => "vtt",
            "mov_text" | "tx3g" => "srt",
            "ttml" | "dfxp" => "ttml",
            "microdvd" => "sub",
            _ => "ass",
        }
    }
}

// Extract subtitle stream to a file
pub async fn extract_subtitle(
    input: &PathBuf,
    subtitle_index: i32,
    output_path: &PathBuf,
    codec: &str,
) -> Result<()> {
    // Check if this is a bitmap subtitle (PGS, VobSub, DVB)
    if is_bitmap_subtitle(codec) {
        return extract_bitmap_subtitle(input, subtitle_index, output_path, codec).await;
    }

    // Determine output format based on codec for text-based subtitles
    let format = match codec.to_lowercase().as_str() {
        "ass" | "ssa" => "ass",
        "subrip" | "srt" => "srt",
        "webvtt" | "vtt" => "webvtt",
        "mov_text" | "tx3g" => "srt",
        "ttml" | "dfxp" => "ttml",
        "microdvd" => "srt",
        _ => "ass",
    };

    info!(
        "Extracting text subtitle stream {} as {} to {:?}",
        subtitle_index, format, output_path
    );

    let output = Command::new("ffmpeg")
        .arg("-v")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(input)
        .arg("-map")
        .arg(format!("0:s:{}", subtitle_index))
        .arg("-c:s")
        .arg(format)
        .arg(output_path)
        .output()
        .await
        .context("failed to extract subtitle")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!("Failed to extract subtitle: {}", stderr);
        anyhow::bail!("ffmpeg subtitle extraction failed: {}", stderr);
    }

    Ok(())
}

/// Extract bitmap-based subtitles (PGS, VobSub) using copy codec
pub async fn extract_bitmap_subtitle(
    input: &PathBuf,
    subtitle_index: i32,
    output_path: &PathBuf,
    codec: &str,
) -> Result<()> {
    info!(
        "Extracting bitmap subtitle stream {} (codec: {}) to {:?}",
        subtitle_index, codec, output_path
    );

    // For VobSub, ffmpeg outputs to .idx which generates both .idx and .sub
    let actual_output_path = if is_vobsub_subtitle(codec) {
        let mut idx_path = output_path.clone();
        idx_path.set_extension("idx");
        idx_path
    } else {
        output_path.clone()
    };

    let output = Command::new("ffmpeg")
        .arg("-v")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(input)
        .arg("-map")
        .arg(format!("0:s:{}", subtitle_index))
        .arg("-c:s")
        .arg("copy")
        .arg(&actual_output_path)
        .output()
        .await
        .context("failed to extract bitmap subtitle")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!("Failed to extract bitmap subtitle: {}", stderr);
        anyhow::bail!("ffmpeg bitmap subtitle extraction failed: {}", stderr);
    }

    Ok(())
}

/// Result of VobSub extraction containing paths to both files
#[allow(dead_code)]
pub struct VobSubExtractionResult {
    pub sub_path: PathBuf,
    pub idx_path: PathBuf,
}

/// Extract VobSub subtitle using mkvextract (produces both .idx and .sub files)
#[allow(dead_code)]
pub async fn extract_vobsub_subtitle(
    input: &PathBuf,
    subtitle_index: i32,
    output_dir: &PathBuf,
    track_idx: usize,
) -> Result<VobSubExtractionResult> {
    let idx_filename = format!("track_{}.idx", track_idx);
    let sub_filename = format!("track_{}.sub", track_idx);
    let idx_path = output_dir.join(&idx_filename);
    let sub_path = output_dir.join(&sub_filename);

    let track_id = get_mkv_track_id(input, subtitle_index).await?;

    info!(
        "Extracting VobSub subtitle track {} (stream {}) using mkvextract to {:?}",
        track_id, subtitle_index, idx_path
    );

    let output = Command::new("mkvextract")
        .arg("tracks")
        .arg(input)
        .arg(format!("{}:{}", track_id, idx_path.display()))
        .output()
        .await
        .context("failed to run mkvextract - is mkvtoolnix installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!("Failed to extract VobSub subtitle: {}", stderr);
        anyhow::bail!("mkvextract failed: {}", stderr);
    }

    if !idx_path.exists() {
        anyhow::bail!("VobSub .idx file was not created: {:?}", idx_path);
    }
    if !sub_path.exists() {
        anyhow::bail!("VobSub .sub file was not created: {:?}", sub_path);
    }

    info!(
        "Successfully extracted VobSub: {:?} and {:?}",
        idx_path, sub_path
    );

    Ok(VobSubExtractionResult { sub_path, idx_path })
}

/// Get the absolute MKV track ID for a subtitle stream index
async fn get_mkv_track_id(input: &PathBuf, subtitle_index: i32) -> Result<i32> {
    let output = Command::new("mkvmerge")
        .arg("-J")
        .arg(input)
        .output()
        .await
        .context("failed to run mkvmerge - is mkvtoolnix installed?")?;

    if !output.status.success() {
        anyhow::bail!("mkvmerge identify failed");
    }

    let info: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("failed to parse mkvmerge output")?;

    let tracks = info["tracks"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("No tracks in mkvmerge output"))?;

    let mut sub_count = 0;
    for track in tracks {
        if track["type"].as_str() == Some("subtitles") {
            if sub_count == subtitle_index {
                return track["id"]
                    .as_i64()
                    .map(|id| id as i32)
                    .ok_or_else(|| anyhow::anyhow!("Track has no ID"));
            }
            sub_count += 1;
        }
    }

    anyhow::bail!("Subtitle stream {} not found in MKV", subtitle_index)
}

// Extract all attachments from a video file to a directory
pub async fn extract_all_attachments(input: &PathBuf, output_dir: &PathBuf) -> Result<()> {
    fs::create_dir_all(output_dir).await?;

    info!("Extracting all attachments to {:?}", output_dir);

    // Use -dump_attachment:t:all to extract all attachments
    let output = Command::new("ffmpeg")
        .arg("-v")
        .arg("error")
        .arg("-y")
        .arg("-dump_attachment:t")
        .arg("")
        .arg("-i")
        .arg(input)
        .current_dir(output_dir)
        .output()
        .await
        .context("failed to extract attachments")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("FFmpeg attachment extraction message: {}", stderr);
        // Don't fail - attachments might still be extracted
    }

    Ok(())
}

pub fn get_variants_for_height(original_height: u32) -> Vec<VideoVariant> {
    let resolution_tiers: [(u32, &str); 6] = [
        (360, "360p"),
        (480, "480p"),
        (720, "720p"),
        (1080, "1080p"),
        (1440, "1440p"),
        (2160, "4K"),
    ];

    // Generate variants dynamically with calculated bitrates
    let mut variants: Vec<VideoVariant> = resolution_tiers
        .iter()
        .filter(|(height, _)| *height <= original_height)
        .map(|(height, label)| VideoVariant::new(label, *height))
        .collect();

    // If no standard tier fits (video shorter than 360p), use original height
    if variants.is_empty() {
        let label = format!("{}p", original_height);
        variants.push(VideoVariant::new(&label, original_height));
    }

    variants
}

#[derive(Debug, Clone, PartialEq)]
enum EncoderType {
    Nvenc,
    Amf,
    Vaapi,
    Qsv,
    Cpu,
}

impl EncoderType {
    fn from_string(s: &str) -> Self {
        if s.contains("nvenc") {
            EncoderType::Nvenc
        } else if s.contains("amf") {
            EncoderType::Amf
        } else if s.contains("vaapi") {
            EncoderType::Vaapi
        } else if s.contains("qsv") {
            EncoderType::Qsv
        } else {
            EncoderType::Cpu
        }
    }

    /// Get the video codec name for this encoder type
    fn video_codec(&self) -> &'static str {
        match self {
            EncoderType::Nvenc => "h264_nvenc",
            EncoderType::Amf => "h264_amf",
            EncoderType::Vaapi => "h264_vaapi",
            EncoderType::Qsv => "h264_qsv",
            EncoderType::Cpu => "libx264",
        }
    }
}

/// Check if an FFmpeg error indicates hardware encoder failure that should fallback to CPU
fn is_hardware_encoder_error(stderr: &str) -> bool {
    let hw_error_patterns = [
        // NVENC specific errors
        "Hardware is lacking required capabilities",
        "Provided device doesn't support required NVENC features",
        "NVENC features",
        "hwaccel initialisation returned error",
        "Failed setup for format cuda",
        "Failed setup for format vaapi",
        "Failed setup for format qsv",
        "Impossible to convert between the formats",
        "Cannot open the hw device",
        "Could not open encoder before EOF",
        "Error initializing the hwcontext",
        "No capable adapters found",
        "Device creation failed",
        "No VAAPI support",
        "DRM setup failed",
        // AMF specific errors
        "AMF failed",
        "Failed to initialise AMF",
        "No AMD devices found",
        // Generic encoder errors that suggest hardware incompatibility
        "Error while opening encoder",
        "maybe incorrect parameters",
        "Error sending frames to consumers",
        "Function not implemented",
        // 10-bit / HDR related
        "Incompatible pixel format",
        "does not support the pixel format",
    ];

    hw_error_patterns
        .iter()
        .any(|pattern| stderr.contains(pattern))
}

/// Get human-readable language name
#[allow(dead_code)]
fn get_language_display_name(lang_code: &str) -> String {
    match lang_code.to_lowercase().as_str() {
        "eng" | "en" => "English".to_string(),
        "jpn" | "ja" => "Japanese".to_string(),
        "spa" | "es" => "Spanish".to_string(),
        "fra" | "fr" => "French".to_string(),
        "deu" | "de" => "German".to_string(),
        "ita" | "it" => "Italian".to_string(),
        "por" | "pt" => "Portuguese".to_string(),
        "rus" | "ru" => "Russian".to_string(),
        "kor" | "ko" => "Korean".to_string(),
        "zho" | "zh" | "chi" => "Chinese".to_string(),
        "ara" | "ar" => "Arabic".to_string(),
        "hin" | "hi" => "Hindi".to_string(),
        "tha" | "th" => "Thai".to_string(),
        "vie" | "vi" => "Vietnamese".to_string(),
        "ind" | "id" => "Indonesian".to_string(),
        "und" => "Unknown".to_string(),
        other => other.to_uppercase(),
    }
}

pub async fn encode_to_hls(
    input: &PathBuf,
    out_dir: &PathBuf,
    progress: &ProgressMap,
    upload_id: &str,
    encoder: &str,
    duration: u32,
    audio_streams: &[AudioStreamInfo],
) -> Result<()> {
    fs::create_dir_all(out_dir).await?;

    // Get original video height to determine appropriate variants
    let (original_height, _) = get_video_metadata(input).await?;
    let variants = get_variants_for_height(original_height);

    if variants.is_empty() {
        anyhow::bail!("No suitable variants for video height {}", original_height);
    }

    let encoder_type = EncoderType::from_string(encoder);

    // GOP size - use 48 for 24fps content (2 seconds), adjust for HLS segment alignment
    let gop = 48;

    let input = input.clone();
    let out_dir = out_dir.clone();
    let upload_id = upload_id.to_string();
    let audio_streams = audio_streams.to_vec();

    // Total tasks = video variants + audio streams + thumbnail + sprites
    let total_steps = variants.len() as u32 + audio_streams.len() as u32 + 2;

    // Timeout heuristic: long enough for slow encodes, but not infinite.
    //  - minimum 30 minutes
    //  - ~20x realtime based on duration
    //  - cap at 6 hours per ffmpeg invocation
    let ffmpeg_timeout = Duration::from_secs((duration as u64).saturating_mul(20).max(30 * 60))
        .min(Duration::from_secs(6 * 60 * 60));

    // Helper that runs ffmpeg and kills it on timeout.
    async fn run_ffmpeg_with_timeout(
        mut cmd: Command,
        timeout_duration: Duration,
        context_label: &str,
    ) -> Result<std::process::Output> {
        cmd.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().context("failed to spawn ffmpeg")?;

        // Capture stderr without moving `child` (wait_with_output consumes Child).
        let mut stderr_handle = child.stderr.take();
        let mut stderr = Vec::new();

        tokio::select! {
            status = child.wait() => {
                let status = status.context("failed to wait for ffmpeg")?;
                if let Some(ref mut err) = stderr_handle {
                    let _ = err.read_to_end(&mut stderr).await;
                }
                Ok(std::process::Output { status, stdout: Vec::new(), stderr })
            }
            _ = sleep(timeout_duration) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                if let Some(ref mut err) = stderr_handle {
                    let _ = err.read_to_end(&mut stderr).await;
                }
                anyhow::bail!("ffmpeg timed out during {context_label} after {:?}", timeout_duration);
            }
        }
    }

    // Encode video variants sequentially (avoids spawning many tasks for large batches).
    for (index, variant) in variants.clone().iter().enumerate() {
        let seg_dir = out_dir.join(&variant.label);
        fs::create_dir_all(&seg_dir).await?;
        let playlist_path = seg_dir.join("index.m3u8");
        let segment_pattern = seg_dir.join("segment_%03d.ts");

        info!(
            "Encoding variant: {} at {}p with bitrate {}kbps (max: {}kbps)",
            variant.label,
            variant.height,
            variant.bitrate,
            variant.max_bitrate()
        );

        // Update progress before starting this variant
        let current_chunk = (index + 1) as u32;
        let percentage = ((current_chunk as f32 / total_steps as f32) * 100.0) as u32;
        let (existing_video_name, existing_created_at) = {
            let progress_map = progress.read().await;
            progress_map
                .get(&upload_id)
                .map(|p| (p.video_name.clone(), p.created_at))
                .unwrap_or((None, 0))
        };
        let start_progress = ProgressUpdate {
            stage: "FFmpeg processing".to_string(),
            current_chunk,
            total_chunks: total_steps,
            percentage,
            details: Some(format!(
                "Encoding variant: {} ({}p)",
                variant.label, variant.height
            )),
            status: "processing".to_string(),
            result: None,
            error: None,
            video_name: existing_video_name.clone(),
            created_at: existing_created_at,
            variant_percentage: None,
        };
        progress
            .write()
            .await
            .insert(upload_id.clone(), start_progress);

        // Try encoding with configured encoder, fallback to CPU if hardware fails
        let mut current_encoder = encoder_type.clone();
        let mut last_error: Option<String> = None;

        loop {
            // Clean up any partial output from previous attempt
            if last_error.is_some() {
                let _ = fs::remove_dir_all(&seg_dir).await;
                fs::create_dir_all(&seg_dir).await?;
            }

            let mut cmd = Command::new("ffmpeg");
            cmd.arg("-loglevel").arg("error").arg("-y");

            // Hardware acceleration setup
            match current_encoder {
                EncoderType::Nvenc => {
                    cmd.arg("-hwaccel")
                        .arg("cuda")
                        .arg("-hwaccel_output_format")
                        .arg("cuda");
                }
                EncoderType::Amf => {
                    cmd.arg("-hwaccel")
                        .arg("d3d11va")
                        .arg("-hwaccel_output_format")
                        .arg("d3d11");
                }
                EncoderType::Vaapi => {
                    cmd.arg("-hwaccel")
                        .arg("vaapi")
                        .arg("-hwaccel_output_format")
                        .arg("vaapi")
                        .arg("-vaapi_device")
                        .arg("/dev/dri/renderD128");
                }
                EncoderType::Qsv => {
                    cmd.arg("-hwaccel")
                        .arg("qsv")
                        .arg("-hwaccel_output_format")
                        .arg("qsv");
                }
                EncoderType::Cpu => {}
            }

            cmd.arg("-i").arg(&input);

            // Scaling filter
            let scale_filter = match current_encoder {
                EncoderType::Nvenc => format!("scale_cuda=-2:{}", variant.height),
                EncoderType::Amf => format!("scale=-2:{}", variant.height), // AMF uses software scale
                EncoderType::Vaapi => format!("scale_vaapi=-2:{}", variant.height),
                EncoderType::Qsv => format!("vpp_qsv=w=-2:h={}", variant.height),
                EncoderType::Cpu => format!("scale=-2:{}", variant.height),
            };

            cmd.arg("-c:v").arg(current_encoder.video_codec());

            // Encoder specific settings
            match current_encoder {
                EncoderType::Nvenc => {
                    cmd.arg("-preset")
                        .arg("p3")
                        .arg("-profile:v")
                        .arg("main")
                        .arg("-level:v")
                        .arg("4.1")
                        .arg("-rc:v")
                        .arg("vbr")
                        .arg("-rc-lookahead")
                        .arg("20")
                        .arg("-bf")
                        .arg("3")
                        .arg("-spatial-aq")
                        .arg("1")
                        .arg("-temporal-aq")
                        .arg("1")
                        .arg("-aq-strength")
                        .arg("8");
                }
                EncoderType::Amf => {
                    cmd.arg("-quality")
                        .arg("balanced")
                        .arg("-profile:v")
                        .arg("main")
                        .arg("-level")
                        .arg("4.1")
                        .arg("-rc")
                        .arg("vbr_latency")
                        .arg("-bf")
                        .arg("3");
                }
                EncoderType::Vaapi => {
                    cmd.arg("-compression_level")
                        .arg("20")
                        .arg("-rc_mode")
                        .arg("VBR")
                        .arg("-profile:v")
                        .arg("main");
                }
                EncoderType::Qsv => {
                    cmd.arg("-preset")
                        .arg("faster")
                        .arg("-profile:v")
                        .arg("main")
                        .arg("-look_ahead")
                        .arg("1")
                        .arg("-look_ahead_depth")
                        .arg("40");
                }
                EncoderType::Cpu => {
                    cmd.arg("-preset")
                        .arg("veryfast")
                        .arg("-profile:v")
                        .arg("main")
                        .arg("-level:v")
                        .arg("4.0");
                }
            }

            cmd.arg("-b:v")
                .arg(variant.bitrate_str())
                .arg("-maxrate")
                .arg(format!("{}k", variant.max_bitrate()))
                .arg("-bufsize")
                .arg(format!("{}k", variant.bufsize()))
                .arg("-vf")
                .arg(&scale_filter);

            if matches!(current_encoder, EncoderType::Cpu) {
                cmd.arg("-pix_fmt").arg("yuv420p");
            }

            cmd.arg("-g")
                .arg(gop.to_string())
                .arg("-keyint_min")
                .arg(gop.to_string())
                .arg("-sc_threshold")
                .arg("0")
                .arg("-force_key_frames")
                .arg("expr:gte(t,n_forced*4)");

            // Don't include audio in video variants - audio is encoded separately
            cmd.arg("-an");

            // Don't include subtitles in HLS output - they are extracted separately
            cmd.arg("-sn");

            cmd.arg("-hls_time")
                .arg("4")
                .arg("-hls_list_size")
                .arg("0")
                .arg("-hls_playlist_type")
                .arg("vod")
                .arg("-hls_segment_type")
                .arg("mpegts")
                .arg("-start_number")
                .arg("0")
                .arg("-hls_segment_filename")
                .arg(&segment_pattern)
                .arg(&playlist_path);

            let output = run_ffmpeg_with_timeout(
                cmd,
                ffmpeg_timeout,
                &format!("encoding variant {}", variant.label),
            )
            .await?;

            if output.status.success() {
                break;
            }

            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            // Check if this is a hardware encoder error and we can fallback
            if current_encoder != EncoderType::Cpu && is_hardware_encoder_error(&stderr) {
                warn!(
                    "Hardware encoder {:?} failed for variant {}, falling back to CPU: {}",
                    current_encoder,
                    variant.label,
                    stderr.lines().next().unwrap_or(&stderr)
                );
                current_encoder = EncoderType::Cpu;
                last_error = Some(stderr);

                // Update progress to indicate fallback
                let fallback_progress = ProgressUpdate {
                    stage: "FFmpeg processing".to_string(),
                    current_chunk,
                    total_chunks: total_steps,
                    percentage,
                    details: Some(format!(
                        "Encoding variant: {} ({}p) - using CPU fallback",
                        variant.label, variant.height
                    )),
                    status: "processing".to_string(),
                    result: None,
                    error: None,
                    video_name: existing_video_name.clone(),
                    created_at: existing_created_at,
                    variant_percentage: None,
                };
                progress
                    .write()
                    .await
                    .insert(upload_id.clone(), fallback_progress);

                continue;
            }

            // Non-recoverable error
            error!("FFmpeg failed for variant {}: {}", variant.label, stderr);
            anyhow::bail!(
                "ffmpeg exited with status: {} for variant {}",
                output.status,
                variant.label
            );
        }

        // Update progress for this variant
        let updated_progress = ProgressUpdate {
            stage: "FFmpeg processing".to_string(),
            current_chunk,
            total_chunks: total_steps,
            percentage,
            details: Some(format!("Encoded variant: {}", variant.label)),
            status: "processing".to_string(),
            result: None,
            error: None,
            video_name: existing_video_name,
            created_at: existing_created_at,
            variant_percentage: None,
        };
        progress
            .write()
            .await
            .insert(upload_id.clone(), updated_progress);
    }

    // Encode each audio stream as a separate HLS audio playlist (sequential)
    let num_video_variants = variants.len();
    for (audio_idx, audio_stream) in audio_streams.iter().enumerate() {
        let audio_label = audio_stream
            .language
            .clone()
            .unwrap_or_else(|| format!("track_{}", audio_idx));
        let audio_dir = out_dir.join(format!("audio_{}", audio_label));
        fs::create_dir_all(&audio_dir).await?;
        let playlist_path = audio_dir.join("index.m3u8");
        let segment_pattern = audio_dir.join("segment_%03d.ts");

        info!(
            "Encoding audio track {}: {} (codec: {}, channels: {:?})",
            audio_idx, audio_label, audio_stream.codec_name, audio_stream.channels
        );

        let current_chunk = (num_video_variants + audio_idx + 1) as u32;
        let percentage = ((current_chunk as f32 / total_steps as f32) * 100.0) as u32;
        let (existing_video_name, existing_created_at) = {
            let progress_map = progress.read().await;
            progress_map
                .get(&upload_id)
                .map(|p| (p.video_name.clone(), p.created_at))
                .unwrap_or((None, 0))
        };
        let audio_progress = ProgressUpdate {
            stage: "FFmpeg processing".to_string(),
            current_chunk,
            total_chunks: total_steps,
            percentage,
            details: Some(format!("Encoding audio track: {}", audio_label)),
            status: "processing".to_string(),
            result: None,
            error: None,
            video_name: existing_video_name.clone(),
            created_at: existing_created_at,
            variant_percentage: None,
        };
        progress
            .write()
            .await
            .insert(upload_id.clone(), audio_progress);

        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-loglevel")
            .arg("error")
            .arg("-y")
            .arg("-i")
            .arg(&input)
            .arg("-map")
            .arg(format!("0:a:{}", audio_idx))
            .arg("-vn")
            .arg("-c:a")
            .arg("aac")
            .arg("-b:a")
            .arg("128k")
            .arg("-ac")
            .arg(if audio_stream.channels.unwrap_or(2) <= 2 {
                audio_stream.channels.unwrap_or(2).to_string()
            } else {
                "2".to_string()
            })
            .arg("-hls_time")
            .arg("4")
            .arg("-hls_list_size")
            .arg("0")
            .arg("-hls_playlist_type")
            .arg("vod")
            .arg("-hls_segment_type")
            .arg("mpegts")
            .arg("-start_number")
            .arg("0")
            .arg("-hls_segment_filename")
            .arg(&segment_pattern)
            .arg(&playlist_path);

        let output = run_ffmpeg_with_timeout(
            cmd,
            ffmpeg_timeout,
            &format!("encoding audio track {}", audio_label),
        )
        .await
        .context("failed to run ffmpeg for audio")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!(
                "FFmpeg audio encoding failed for track {}: {}",
                audio_idx, stderr
            );
            anyhow::bail!(
                "ffmpeg audio encoding failed for track {}: {}",
                audio_idx,
                stderr
            );
        }

        info!("Audio track {} encoded successfully", audio_label);
    }

    // Generate thumbnail (single frame at 10% of video)
    {
        let thumbnail_path = out_dir.join("thumbnail.jpg");
        info!("Generating thumbnail: {:?}", thumbnail_path);

        let seek_time = (duration as f64 * 0.1).max(1.0);

        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-loglevel")
            .arg("error")
            .arg("-y")
            .arg("-ss")
            .arg(format!("{}", seek_time))
            .arg("-i")
            .arg(&input)
            .arg("-vf")
            .arg("scale=480:-1")
            .arg("-frames:v")
            .arg("1")
            .arg("-q:v")
            .arg("2")
            .arg(&thumbnail_path);

        match run_ffmpeg_with_timeout(cmd, ffmpeg_timeout, "generating thumbnail").await {
            Ok(output) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!("Thumbnail generation failed: {}", stderr);
                }
            }
            Err(e) => {
                error!("Thumbnail generation failed: {}", e);
            }
        }
    }

    // Generate sprites (preview thumbnails grid)
    {
        let sprite_path = out_dir.join("sprites.jpg");
        info!("Generating thumbnail sprite: {:?}", sprite_path);

        let target_frames = 100.0;
        let fps = if duration > 0 {
            (target_frames / duration as f64).max(0.01)
        } else {
            1.0
        };

        let vf_filter = format!("fps={:.4},scale=160:-1,tile=10x10", fps);

        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-loglevel")
            .arg("error")
            .arg("-y")
            .arg("-i")
            .arg(&input)
            .arg("-vf")
            .arg(&vf_filter)
            .arg("-frames:v")
            .arg("1")
            .arg("-q:v")
            .arg("5")
            .arg(&sprite_path);

        match run_ffmpeg_with_timeout(cmd, ffmpeg_timeout, "generating sprites").await {
            Ok(output) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!("Thumbnail sprite generation failed: {}", stderr);
                }
            }
            Err(e) => {
                error!("Thumbnail sprite generation failed: {}", e);
            }
        }
    }

    // Create master playlist with audio track support
    let master_playlist_path = out_dir.join("index.m3u8");
    let mut master_content = String::from("#EXTM3U\n#EXT-X-VERSION:3\n\n");

    let variants_ref = get_variants_for_height(get_video_height(&input).await?);

    // Add audio tracks as EXT-X-MEDIA entries
    if !audio_streams.is_empty() {
        for (idx, audio) in audio_streams.iter().enumerate() {
            let audio_label = audio
                .language
                .clone()
                .unwrap_or_else(|| format!("track_{}", idx));
            let language = audio.language.as_deref().unwrap_or("und");
            let name = audio
                .title
                .clone()
                .unwrap_or_else(|| get_language_display_name(language));
            let is_default = if audio.is_default || idx == 0 {
                "YES"
            } else {
                "NO"
            };
            let autoselect = if audio.is_default || idx == 0 {
                "YES"
            } else {
                "NO"
            };

            master_content.push_str(&format!(
                "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",LANGUAGE=\"{}\",NAME=\"{}\",DEFAULT={},AUTOSELECT={},URI=\"audio_{}/index.m3u8\"\n",
                language,
                name,
                is_default,
                autoselect,
                audio_label
            ));
        }
        master_content.push('\n');
    }

    // Add video stream variants with audio group reference
    for variant in &variants_ref {
        let audio_group = if !audio_streams.is_empty() {
            ",AUDIO=\"audio\""
        } else {
            ""
        };

        let stream_inf = format!(
            "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}x{}{}\n",
            variant.bandwidth(),
            (((variant.height as f32) * 16.0) / 9.0) as u32,
            variant.height,
            audio_group
        );

        master_content.push_str(&stream_inf);
        master_content.push_str(&format!("{}/index.m3u8\n", variant.label));
    }

    fs::write(&master_playlist_path, master_content)
        .await
        .context("failed to write master playlist")?;

    Ok(())
}
