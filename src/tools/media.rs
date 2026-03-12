//! Media Tool — YouTube search, playback, and audio download via yt-dlp.
//!
//! Ported from `sovereign_titan/tools/media_processing.py`. Wraps the
//! `yt-dlp` CLI as a subprocess for searching, playing, downloading, and
//! querying metadata for YouTube videos.

use anyhow::Result;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;
use tracing::info;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Timeout for download operations.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// Timeout for search / info operations.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Media directory for downloaded files.
const MEDIA_DIR: &str = "workspace/media";

/// Media processing tool wrapping yt-dlp.
pub struct MediaTool;

impl MediaTool {
    /// Run a yt-dlp command and return stdout.
    fn run_ytdlp(args: &[&str], timeout: Duration) -> Result<String> {
        let mut cmd = std::process::Command::new("yt-dlp");
        cmd.args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        #[cfg(windows)]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

        let child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!(
                "Failed to launch yt-dlp (is it installed?): {e}"
            )
        })?;

        let output = wait_with_timeout(child, timeout)?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() && stdout.trim().is_empty() {
            anyhow::bail!(
                "yt-dlp exited with {}: {}",
                output.status,
                &stderr[..stderr.len().min(300)]
            );
        }

        Ok(stdout)
    }

    /// Search YouTube and return titles + URLs.
    fn search(query: &str, count: u32) -> Result<String> {
        let search_term = format!("ytsearch{count}:{query}");
        let stdout = Self::run_ytdlp(
            &["--dump-json", "--flat-playlist", &search_term],
            SEARCH_TIMEOUT,
        )?;

        let mut results = Vec::new();
        for (i, line) in stdout.lines().enumerate() {
            if let Ok(entry) = serde_json::from_str::<Value>(line) {
                let title = entry
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(untitled)");
                let url = entry
                    .get("url")
                    .or_else(|| entry.get("webpage_url"))
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| {
                        // Build URL from id if available.
                        entry
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(|id| format!("https://www.youtube.com/watch?v={id}"))
                            .unwrap_or_default()
                    });
                let duration = entry
                    .get("duration")
                    .and_then(|v| v.as_f64())
                    .map(|d| format_duration(d as u64))
                    .unwrap_or_default();

                results.push(format!("{}. {} [{}]\n   {}", i + 1, title, duration, url));
            }
        }

        if results.is_empty() {
            Ok(format!("No results found for: {query}"))
        } else {
            Ok(format!(
                "YouTube search: {query}\n\n{}",
                results.join("\n\n")
            ))
        }
    }

    /// Search and open the first result in the default browser.
    fn play(query: &str) -> Result<String> {
        let search_term = format!("ytsearch1:{query}");
        let stdout = Self::run_ytdlp(
            &["--dump-json", "--flat-playlist", &search_term],
            SEARCH_TIMEOUT,
        )?;

        let first_line = stdout.lines().next().unwrap_or("");
        let entry: Value = serde_json::from_str(first_line)
            .map_err(|_| anyhow::anyhow!("No results found for: {query}"))?;

        let title = entry
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("(untitled)");
        let url = entry
            .get("url")
            .or_else(|| entry.get("webpage_url"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| {
                entry
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|id| format!("https://www.youtube.com/watch?v={id}"))
                    .unwrap_or_default()
            });

        if url.is_empty() {
            return Ok(format!("Found '{title}' but could not determine URL."));
        }

        open::that(&url).map_err(|e| anyhow::anyhow!("Failed to open browser: {e}"))?;

        Ok(format!("Playing: {title}\n{url}"))
    }

    /// Download audio as MP3 from a URL.
    fn download_audio(url: &str) -> Result<String> {
        // Ensure media directory exists.
        let media_dir = PathBuf::from(MEDIA_DIR);
        std::fs::create_dir_all(&media_dir)?;

        let output_template = format!("{}/%(title)s.%(ext)s", MEDIA_DIR);
        let stdout = Self::run_ytdlp(
            &[
                "-x",
                "--audio-format",
                "mp3",
                "--audio-quality",
                "192K",
                "-o",
                &output_template,
                url,
            ],
            DOWNLOAD_TIMEOUT,
        )?;

        // Parse the output to find the destination file.
        let dest = stdout
            .lines()
            .find(|line| line.contains("[ExtractAudio] Destination:") || line.contains("has already been downloaded"))
            .map(|line| line.trim().to_string())
            .unwrap_or_else(|| "Download completed.".to_string());

        // Try to find the actual file.
        let files: Vec<_> = std::fs::read_dir(&media_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| ext == "mp3")
            })
            .collect();

        if let Some(latest) = files.iter().max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok())) {
            let size = latest.metadata().map(|m| m.len()).unwrap_or(0);
            let size_mb = size as f64 / (1024.0 * 1024.0);
            Ok(format!(
                "Downloaded: {}\nSize: {:.1} MB\n{}",
                latest.path().display(),
                size_mb,
                dest
            ))
        } else {
            Ok(dest)
        }
    }

    /// Get video metadata.
    fn video_info(url: &str) -> Result<String> {
        let stdout = Self::run_ytdlp(&["--dump-json", url], SEARCH_TIMEOUT)?;

        let data: Value = serde_json::from_str(stdout.lines().next().unwrap_or("{}"))
            .unwrap_or_default();

        let title = data
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)");
        let duration = data
            .get("duration")
            .and_then(|v| v.as_f64())
            .map(|d| format_duration(d as u64))
            .unwrap_or_else(|| "?".to_string());
        let views = data
            .get("view_count")
            .and_then(|v| v.as_u64())
            .map(format_views)
            .unwrap_or_else(|| "?".to_string());
        let uploader = data
            .get("uploader")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let description = data
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .chars()
            .take(500)
            .collect::<String>();

        Ok(format!(
            "Title: {title}\nDuration: {duration}\nViews: {views}\nUploader: {uploader}\n\nDescription:\n{description}"
        ))
    }
}

#[async_trait::async_trait]
impl super::Tool for MediaTool {
    fn name(&self) -> &'static str {
        "media"
    }

    fn description(&self) -> &'static str {
        "YouTube media tool (search, play, download audio, get info). \
         Input: {\"action\": \"search\", \"query\": \"lofi beats\"} — searches YouTube. \
         {\"action\": \"play\", \"query\": \"never gonna give you up\"} — plays first result. \
         {\"action\": \"download_audio\", \"url\": \"https://...\"} — downloads MP3. \
         {\"action\": \"info\", \"url\": \"https://...\"} — shows video metadata. \
         Requires yt-dlp installed."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("search");

        info!("media tool: action={action}");

        let input_clone = input.clone();
        let action_owned = action.to_string();

        let result = tokio::task::spawn_blocking(move || {
            match action_owned.as_str() {
                "search" => {
                    let query = input_clone
                        .get("query")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if query.is_empty() {
                        return Ok("Error: search requires a \"query\" field.".to_string());
                    }
                    let count = input_clone
                        .get("count")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(5) as u32;
                    Self::search(query, count)
                }
                "play" => {
                    let query = input_clone
                        .get("query")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if query.is_empty() {
                        return Ok("Error: play requires a \"query\" field.".to_string());
                    }
                    Self::play(query)
                }
                "download_audio" => {
                    let url = input_clone
                        .get("url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if url.is_empty() {
                        return Ok("Error: download_audio requires a \"url\" field.".to_string());
                    }
                    Self::download_audio(url)
                }
                "info" => {
                    let url = input_clone
                        .get("url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if url.is_empty() {
                        return Ok("Error: info requires a \"url\" field.".to_string());
                    }
                    Self::video_info(url)
                }
                other => Ok(format!(
                    "Unknown action: '{other}'. Use: search, play, download_audio, info."
                )),
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking failed: {e}"))??;

        Ok(result)
    }
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

/// Format view count with K/M suffixes.
fn format_views(views: u64) -> String {
    if views >= 1_000_000 {
        format!("{:.1}M", views as f64 / 1_000_000.0)
    } else if views >= 1_000 {
        format!("{:.1}K", views as f64 / 1_000.0)
    } else {
        format!("{views}")
    }
}

/// Wait for a child process with a timeout.
fn wait_with_timeout(
    child: std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output> {
    // On stable Rust we use a thread to enforce timeout.
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let result = child.wait_with_output();
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(result) => result.map_err(|e| anyhow::anyhow!("yt-dlp process error: {e}")),
        Err(_) => anyhow::bail!("yt-dlp timed out after {}s", timeout.as_secs()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;
    use serde_json::json;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(65), "1:05");
        assert_eq!(format_duration(3661), "1:01:01");
        assert_eq!(format_duration(0), "0:00");
    }

    #[test]
    fn test_format_views() {
        assert_eq!(format_views(1_500_000), "1.5M");
        assert_eq!(format_views(2_500), "2.5K");
        assert_eq!(format_views(500), "500");
    }

    #[tokio::test]
    async fn test_unknown_action() {
        let tool = MediaTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_search_missing_query() {
        let tool = MediaTool;
        let result = tool.execute(json!({"action": "search"})).await.unwrap();
        assert!(result.contains("Error"));
    }

    #[tokio::test]
    async fn test_download_missing_url() {
        let tool = MediaTool;
        let result = tool
            .execute(json!({"action": "download_audio"}))
            .await
            .unwrap();
        assert!(result.contains("Error"));
    }
}
