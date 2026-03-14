//! Inter-Process Communication protocol for isolated model workers.
//!
//! Subconscious and Warden models run in separate OS processes to avoid
//! GGML C-state conflicts. Communication uses JSON-over-stdio:
//!
//! - Main process writes JSON commands to worker's stdin
//! - Worker writes JSON responses to its stdout
//! - One JSON object per line (newline-delimited JSON)

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Worker request / response protocol
// ─────────────────────────────────────────────────────────────────────────────

/// A command sent from the main Tauri process to a worker subprocess.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WorkerRequest {
    /// Generate text from a prompt.
    Generate {
        prompt: String,
        max_tokens: u32,
        temperature: f32,
    },
    /// Graceful shutdown.
    Shutdown,
    /// Health check — worker should respond with `WorkerResponse::Ok`.
    Ping,
}

/// A response from a worker subprocess back to the main process.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WorkerResponse {
    /// Successful generation result.
    Generated { text: String, tokens: u32 },
    /// Worker is alive and ready.
    Ok,
    /// An error occurred during generation.
    Error { message: String },
}

/// Which worker role this process serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerRole {
    Subconscious,
    Warden,
}

impl WorkerRole {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "subconscious" => Some(Self::Subconscious),
            "warden" => Some(Self::Warden),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Subconscious => "subconscious",
            Self::Warden => "warden",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Worker process handle (used by the main process)
// ─────────────────────────────────────────────────────────────────────────────

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// Handle to a spawned worker subprocess.
///
/// Provides typed send/receive over JSON-over-stdio.
pub struct WorkerHandle {
    pub role: WorkerRole,
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
}

impl WorkerHandle {
    /// Spawn a worker subprocess.
    ///
    /// The worker binary is expected at the same directory as the main
    /// executable, named `titan_worker.exe` (Windows) or `titan_worker`.
    pub fn spawn(role: WorkerRole) -> anyhow::Result<Self> {
        let exe = std::env::current_exe()?;
        let worker_path = exe.parent().unwrap().join(if cfg!(windows) {
            "titan_worker.exe"
        } else {
            "titan_worker"
        });

        if !worker_path.exists() {
            anyhow::bail!(
                "Worker binary not found at {}. Build with: cargo build --release --bin titan_worker",
                worker_path.display()
            );
        }

        let mut child = Command::new(&worker_path)
            .arg(role.as_str())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // Worker logs go to parent's stderr
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn {} worker: {e}", role.as_str()))?;

        let stdin = child.stdin.take().expect("stdin must be piped");
        let stdout = child.stdout.take().expect("stdout must be piped");
        let reader = BufReader::new(stdout);

        Ok(Self {
            role,
            child,
            stdin,
            reader,
        })
    }

    /// Get the child process ID.
    pub fn child_id(&self) -> u32 {
        self.child.id()
    }

    /// Send a request to the worker and wait for the response.
    pub fn send(&mut self, request: &WorkerRequest) -> anyhow::Result<WorkerResponse> {
        let json = serde_json::to_string(request)?;
        writeln!(self.stdin, "{json}")?;
        self.stdin.flush()?;

        let mut line = String::new();
        self.reader.read_line(&mut line)?;

        if line.is_empty() {
            anyhow::bail!("{} worker closed stdout unexpectedly", self.role.as_str());
        }

        let response: WorkerResponse = serde_json::from_str(line.trim())?;
        Ok(response)
    }

    /// Send a shutdown command to the worker.
    pub fn shutdown(&mut self) {
        let _ = self.send(&WorkerRequest::Shutdown);
        let _ = self.child.wait();
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
