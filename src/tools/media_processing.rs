//! Media Processing Tool — file conversion and metadata via ffmpeg/ffprobe.
//!
//! Provides actions for querying media file information (duration, codec,
//! resolution) and converting between formats. Shells out to ffmpeg and
//! ffprobe, which must be installed and on PATH.

use anyhow::Result;
use serde_json::Value;
use std::time::Duration;
use tracing::info;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Timeout for ffmpeg/ffprobe operations.
const PROCESS_TIMEOUT: Duration = Duration::from_secs(120);

/// Media file processing tool wrapping ffmpeg/ffprobe.
pub struct MediaProcessingTool;

impl MediaProcessingTool {
    /// Run an external command with a timeout and return stdout.
    fn run_command(program: &str, args: &[&str]) -> Result<String, String> {
        let mut cmd = std::process::Command::new(program);
        cmd.args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        #[cfg(windows)]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to launch {program} (is it installed?): {e}"))?;

        let output = wait_with_timeout(child, PROCESS_TIMEOUT)?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() && stdout.trim().is_empty() {
            return Err(format!(
                "{program} exited with {}: {}",
                output.status,
                &stderr[..stderr.len().min(300)]
            ));
        }

        Ok(stdout)
    }

    /// Get media file information using ffprobe.
    fn info(path: &str) -> String {
        if path.is_empty() {
            return "info requires a non-empty \"path\" field.".to_string();
        }

        let file_path = std::path::Path::new(path);
        if !file_path.exists() {
            return format!("File not found: {path}");
        }

        // Use ffprobe to get JSON metadata.
        match Self::run_command(
            "ffprobe",
            &[
                "-v", "quiet",
                "-print_format", "json",
                "-show_format",
                "-show_streams",
                path,
            ],
        ) {
            Ok(stdout) => {
                // Parse the JSON output and extract key information.
                match serde_json::from_str::<Value>(&stdout) {
                    Ok(data) => format_media_info(&data, path),
                    Err(_) => {
                        // Return raw output if JSON parsing fails.
                        format!("Media info for '{path}':\n{stdout}")
                    }
                }
            }
            Err(e) => format!("Failed to get media info: {e}"),
        }
    }

    /// Convert a media file to a different format using ffmpeg.
    fn convert(input: &str, output: &str, format: &str) -> String {
        if input.is_empty() {
            return "convert requires a non-empty \"input\" field.".to_string();
        }
        if output.is_empty() {
            return "convert requires a non-empty \"output\" field.".to_string();
        }

        let input_path = std::path::Path::new(input);
        if !input_path.exists() {
            return format!("Input file not found: {input}");
        }

        // Build ffmpeg args.
        let mut args: Vec<&str> = vec!["-i", input, "-y"]; // -y to overwrite

        // Add format-specific options.
        match format {
            "mp3" => {
                args.extend_from_slice(&["-codec:a", "libmp3lame", "-q:a", "2"]);
            }
            "wav" => {
                args.extend_from_slice(&["-codec:a", "pcm_s16le"]);
            }
            "mp4" => {
                args.extend_from_slice(&["-codec:v", "libx264", "-preset", "medium"]);
            }
            "webm" => {
                args.extend_from_slice(&["-codec:v", "libvpx-vp9", "-codec:a", "libopus"]);
            }
            "flac" => {
                args.extend_from_slice(&["-codec:a", "flac"]);
            }
            "ogg" => {
                args.extend_from_slice(&["-codec:a", "libvorbis"]);
            }
            _ => {
                // Let ffmpeg infer from extension.
            }
        }

        args.push(output);

        match Self::run_command("ffmpeg", &args) {
            Ok(_stdout) => {
                // Check if output file was created.
                let output_path = std::path::Path::new(output);
                if output_path.exists() {
                    let size = std::fs::metadata(output_path)
                        .map(|m| m.len())
                        .unwrap_or(0);
                    format!(
                        "Converted: {input} -> {output} ({:.1} MB)",
                        size as f64 / (1024.0 * 1024.0)
                    )
                } else {
                    format!("Conversion completed but output file not found: {output}")
                }
            }
            Err(e) => format!("Conversion failed: {e}"),
        }
    }
}

/// Format ffprobe JSON output into a human-readable summary.
fn format_media_info(data: &Value, path: &str) -> String {
    let mut output = format!("Media info: {path}\n\n");

    // Format section.
    if let Some(format) = data.get("format") {
        let duration = format
            .get("duration")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .map(|d| format_duration(d as u64))
            .unwrap_or_else(|| "unknown".to_string());
        let size = format
            .get("size")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .map(|s| format!("{:.1} MB", s as f64 / (1024.0 * 1024.0)))
            .unwrap_or_else(|| "unknown".to_string());
        let format_name = format
            .get("format_long_name")
            .or_else(|| format.get("format_name"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let bitrate = format
            .get("bit_rate")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .map(|b| format!("{} kbps", b / 1000))
            .unwrap_or_else(|| "unknown".to_string());

        output.push_str(&format!(
            "Format: {format_name}\nDuration: {duration}\nSize: {size}\nBitrate: {bitrate}\n"
        ));
    }

    // Stream sections.
    if let Some(streams) = data.get("streams").and_then(|v| v.as_array()) {
        for (i, stream) in streams.iter().enumerate() {
            let codec_type = stream
                .get("codec_type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let codec_name = stream
                .get("codec_long_name")
                .or_else(|| stream.get("codec_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            output.push_str(&format!("\nStream #{i} ({codec_type}): {codec_name}"));

            if codec_type == "video" {
                let width = stream.get("width").and_then(|v| v.as_u64()).unwrap_or(0);
                let height = stream.get("height").and_then(|v| v.as_u64()).unwrap_or(0);
                let fps = stream
                    .get("r_frame_rate")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                output.push_str(&format!("\n  Resolution: {width}x{height}\n  FPS: {fps}"));
            } else if codec_type == "audio" {
                let sample_rate = stream
                    .get("sample_rate")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let channels = stream
                    .get("channels")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                output.push_str(&format!(
                    "\n  Sample rate: {sample_rate} Hz\n  Channels: {channels}"
                ));
            }
            output.push('\n');
        }
    }

    output
}

/// Format seconds as HH:MM:SS or MM:SS.
fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Wait for a child process with a timeout.
fn wait_with_timeout(
    child: std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let result = child.wait_with_output();
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(result) => result.map_err(|e| format!("Process error: {e}")),
        Err(_) => Err(format!("Process timed out after {}s", timeout.as_secs())),
    }
}

#[async_trait::async_trait]
impl super::Tool for MediaProcessingTool {
    fn name(&self) -> &'static str {
        "media_processing"
    }

    fn description(&self) -> &'static str {
        "Media file processing via ffmpeg. Actions: \
         info (path) — get duration, codec, resolution; \
         convert (input, output, format?) — convert between formats. \
         Supported formats: mp3, wav, mp4, webm, flac, ogg, and more. \
         Input: {\"action\": \"info\", \"path\": \"video.mp4\"}. \
         Requires ffmpeg/ffprobe installed."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("info");

        info!("media_processing: action={action}");

        let input_clone = input.clone();
        let action_owned = action.to_string();

        let result = tokio::task::spawn_blocking(move || {
            match action_owned.as_str() {
                "info" | "metadata" | "probe" => {
                    let path = input_clone
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    Self::info(path)
                }
                "convert" | "transcode" => {
                    let input_path = input_clone
                        .get("input")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let output_path = input_clone
                        .get("output")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let format = input_clone
                        .get("format")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    Self::convert(input_path, output_path, format)
                }
                other => format!(
                    "Unknown action: '{other}'. Use: info, convert."
                ),
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking failed: {e}"))?;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use serde_json::json;

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = MediaProcessingTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_info_missing_path() {
        let tool = MediaProcessingTool;
        let result = tool
            .execute(json!({"action": "info", "path": ""}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_info_nonexistent_file() {
        let tool = MediaProcessingTool;
        let result = tool
            .execute(json!({"action": "info", "path": "/nonexistent/video.mp4"}))
            .await
            .unwrap();
        assert!(result.contains("not found") || result.contains("Not"));
    }

    #[tokio::test]
    async fn test_convert_missing_input() {
        let tool = MediaProcessingTool;
        let result = tool
            .execute(json!({"action": "convert", "output": "out.mp3"}))
            .await
            .unwrap();
        assert!(result.contains("requires"));
    }

    #[tokio::test]
    async fn test_convert_missing_output() {
        let tool = MediaProcessingTool;
        let result = tool
            .execute(json!({"action": "convert", "input": "in.wav"}))
            .await
            .unwrap();
        assert!(result.contains("requires") || result.contains("not found"));
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(65), "1:05");
        assert_eq!(format_duration(3661), "1:01:01");
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(59), "0:59");
    }

    #[test]
    fn test_tool_name() {
        let tool = MediaProcessingTool;
        assert_eq!(tool.name(), "media_processing");
    }

    #[test]
    fn test_tool_description_not_empty() {
        let tool = MediaProcessingTool;
        assert!(!tool.description().is_empty());
    }
}
