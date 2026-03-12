//! Native Threat Response — instant process termination via Win32 API.
//!
//! Uses `OpenProcess` + `TerminateProcess` directly, bypassing `taskkill.exe`
//! for zero-latency threat neutralization.

#![cfg(windows)]

use anyhow::Result;
use tracing::{info, warn};

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    OpenProcess, TerminateProcess, PROCESS_TERMINATE,
};

/// Terminate a process by PID using native Win32 API.
///
/// This is significantly faster than spawning `taskkill.exe` — the syscall
/// executes in microseconds rather than the ~50ms process-spawn overhead.
///
/// # Arguments
/// * `pid` — Process ID to terminate.
///
/// # Errors
/// Returns an error if the process cannot be opened (insufficient privileges,
/// PID does not exist) or if termination fails.
pub fn terminate_threat(pid: u32) -> Result<()> {
    info!("Terminating threat PID {pid}...");

    // Open the process with TERMINATE permission.
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, false, pid) }
        .map_err(|e| anyhow::anyhow!("OpenProcess({pid}) failed: {e}"))?;

    // Terminate the process with exit code 1.
    let result = unsafe { TerminateProcess(handle, 1) };

    // Always close the handle, even if termination failed.
    unsafe {
        let _ = CloseHandle(handle);
    }

    match result {
        Ok(()) => {
            info!("PID {pid} terminated successfully");
            Ok(())
        }
        Err(e) => {
            warn!("TerminateProcess({pid}) failed: {e}");
            Err(anyhow::anyhow!("TerminateProcess({pid}) failed: {e}"))
        }
    }
}

/// Terminate multiple threat PIDs, logging each result.
///
/// Returns the count of successfully terminated processes.
pub fn terminate_threats(pids: &[u32]) -> usize {
    let mut killed = 0;
    for &pid in pids {
        match terminate_threat(pid) {
            Ok(()) => killed += 1,
            Err(e) => warn!("Failed to kill PID {pid}: {e:#}"),
        }
    }
    info!("Terminated {killed}/{} threat processes", pids.len());
    killed
}
