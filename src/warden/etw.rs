//! Native Windows ETW (Event Tracing for Windows) Listener.
//!
//! Hooks into the `Microsoft-Windows-Crypto-NCrypt` provider to detect
//! cryptographic operations in real-time. When symmetric encryption patterns
//! consistent with ransomware (e.g., AES-CTR bulk encryption) are detected,
//! a [`SecurityAlert`](crate::messages::CognitiveMessage::SecurityAlert) is
//! pushed to the Warden's `mpsc` channel for zero-latency threat response.

#![cfg(windows)]

use std::mem;
use std::ptr;
use std::sync::OnceLock;

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use windows::core::GUID;
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::System::Diagnostics::Etw::{
    CloseTrace, ControlTraceW, EnableTraceEx2, OpenTraceW, ProcessTrace, StartTraceW,
    CONTROLTRACE_HANDLE, EVENT_CONTROL_CODE_ENABLE_PROVIDER, EVENT_RECORD,
    EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES,
    EVENT_TRACE_REAL_TIME_MODE, PROCESSTRACE_HANDLE, PROCESS_TRACE_MODE_EVENT_RECORD,
    PROCESS_TRACE_MODE_REAL_TIME, TRACE_LEVEL_INFORMATION,
};

use crate::messages::CognitiveMessage;

/// Microsoft-Windows-Crypto-NCrypt provider GUID.
const NCRYPT_PROVIDER_GUID: GUID = GUID::from_values(
    0xE3E0_E2F0,
    0xC9C5,
    0x11E0,
    [0x8A, 0xB8, 0x00, 0x24, 0xE8, 0x35, 0x99, 0x15],
);

/// Session name for our ETW trace.
const SESSION_NAME: &str = "TitanCryptoMonitor";

/// ERROR_ALREADY_EXISTS
const ERROR_ALREADY_EXISTS: u32 = 183;

/// Global sender for the ETW callback to push alerts into.
/// ETW callbacks are C-style function pointers — no closures allowed —
/// so we use a `OnceLock` to store the sender.
static ALERT_SENDER: OnceLock<mpsc::UnboundedSender<EtwCryptoEvent>> = OnceLock::new();

/// Helper: check a WIN32_ERROR, converting non-zero to anyhow::Error.
fn check_win32(err: WIN32_ERROR, context: &str) -> anyhow::Result<()> {
    if err.0 == 0 {
        Ok(())
    } else {
        Err(anyhow::anyhow!("{context}: WIN32_ERROR({:#x})", err.0))
    }
}

/// A parsed crypto event from ETW.
#[derive(Debug, Clone)]
pub struct EtwCryptoEvent {
    pub provider_id: GUID,
    pub event_id: u16,
    pub process_id: u32,
    pub thread_id: u32,
    pub timestamp: i64,
}

/// The ETW Monitor — manages the trace session lifecycle.
pub struct EtwMonitor {
    session_handle: CONTROLTRACE_HANDLE,
    trace_handle: Option<PROCESSTRACE_HANDLE>,
    /// Receives parsed crypto events from the ETW callback.
    event_rx: mpsc::UnboundedReceiver<EtwCryptoEvent>,
    /// Sender to push SecurityAlerts to the Prime actor.
    prime_tx: mpsc::Sender<CognitiveMessage>,
}

impl EtwMonitor {
    /// Create and start a new ETW monitor session.
    ///
    /// # Safety
    /// Must be run with Administrator privileges for ETW access.
    pub fn new(prime_tx: mpsc::Sender<CognitiveMessage>) -> anyhow::Result<Self> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        // Store the sender globally for the C callback.
        ALERT_SENDER
            .set(event_tx)
            .map_err(|_| anyhow::anyhow!("ETW alert sender already initialized"))?;

        // Allocate EVENT_TRACE_PROPERTIES with session name buffer.
        let session_name_wide: Vec<u16> = SESSION_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let props_size =
            mem::size_of::<EVENT_TRACE_PROPERTIES>() + (session_name_wide.len() * 2);
        let mut buffer = vec![0u8; props_size];
        let props = unsafe { &mut *(buffer.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES) };

        props.Wnode.BufferSize = props_size as u32;
        props.Wnode.Flags = 0x0002_0000; // WNODE_FLAG_TRACED_GUID
        props.Wnode.ClientContext = 1; // QPC timestamp
        props.LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        props.LoggerNameOffset = mem::size_of::<EVENT_TRACE_PROPERTIES>() as u32;

        // Copy session name into the buffer after the struct.
        let name_dest = unsafe {
            buffer
                .as_mut_ptr()
                .add(mem::size_of::<EVENT_TRACE_PROPERTIES>())
        };
        unsafe {
            ptr::copy_nonoverlapping(
                session_name_wide.as_ptr() as *const u8,
                name_dest,
                session_name_wide.len() * 2,
            );
        }

        // Start the trace session.
        let mut session_handle = CONTROLTRACE_HANDLE::default();
        let session_name_pcwstr = windows::core::PCWSTR(session_name_wide.as_ptr());

        let status = unsafe {
            StartTraceW(&mut session_handle, session_name_pcwstr, props)
        };

        if status.0 != 0 {
            if status.0 == ERROR_ALREADY_EXISTS {
                warn!("ETW session already exists, stopping old session...");
                unsafe {
                    let _ = ControlTraceW(
                        CONTROLTRACE_HANDLE::default(),
                        session_name_pcwstr,
                        props,
                        EVENT_TRACE_CONTROL_STOP,
                    );
                }
                // Retry
                let retry = unsafe {
                    StartTraceW(&mut session_handle, session_name_pcwstr, props)
                };
                check_win32(retry, "StartTraceW retry")?;
            } else {
                check_win32(status, "StartTraceW (run as Administrator)")?;
            }
        }

        info!("ETW session started: {SESSION_NAME}");

        // Enable the NCrypt provider on this session.
        let enable_status = unsafe {
            EnableTraceEx2(
                session_handle,
                &NCRYPT_PROVIDER_GUID,
                EVENT_CONTROL_CODE_ENABLE_PROVIDER.0,
                TRACE_LEVEL_INFORMATION as u8,
                0, // match any keyword
                0,
                0,
                None,
            )
        };
        check_win32(enable_status, "EnableTraceEx2")?;

        info!("ETW provider enabled: Microsoft-Windows-Crypto-NCrypt");

        Ok(Self {
            session_handle,
            trace_handle: None,
            event_rx,
            prime_tx,
        })
    }

    /// Start processing ETW events in a background thread.
    ///
    /// `ProcessTrace` blocks, so this spawns a dedicated OS thread.
    pub fn start_processing(&mut self) -> anyhow::Result<()> {
        let session_name_wide: Vec<u16> = SESSION_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let mut logfile = EVENT_TRACE_LOGFILEW::default();
        logfile.LoggerName = windows::core::PWSTR(session_name_wide.as_ptr() as *mut u16);
        logfile.Anonymous1.ProcessTraceMode =
            PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
        logfile.Anonymous2.EventRecordCallback = Some(etw_event_callback);

        let trace_handle = unsafe { OpenTraceW(&mut logfile) };
        if trace_handle.Value == u64::MAX {
            return Err(anyhow::anyhow!("OpenTraceW failed"));
        }

        self.trace_handle = Some(trace_handle);

        // ProcessTrace blocks — run on a dedicated thread.
        let handle = trace_handle;
        std::thread::Builder::new()
            .name("etw-processor".to_string())
            .spawn(move || {
                info!("ETW processor thread started");
                let handles = [handle];
                let result = unsafe { ProcessTrace(&handles, None, None) };
                if result.0 != 0 {
                    error!("ProcessTrace exited with WIN32_ERROR({:#x})", result.0);
                } else {
                    info!("ProcessTrace exited cleanly");
                }
            })?;

        info!("ETW event processing started");
        Ok(())
    }

    /// Run the event dispatch loop — reads parsed events and sends alerts.
    ///
    /// Call this from an async context (e.g., inside a `tokio::spawn`).
    pub async fn run_dispatch_loop(&mut self) {
        // Track encryption event rate for ransomware detection.
        let mut recent_events: Vec<i64> = Vec::new();
        const RATE_WINDOW_MS: i64 = 5_000; // 5 second window
        const RATE_THRESHOLD: usize = 20; // >20 crypto ops in 5s = suspicious

        while let Some(event) = self.event_rx.recv().await {
            let now_ms = event.timestamp / 10_000; // 100ns ticks → ms
            recent_events.push(now_ms);

            // Prune events outside the window.
            recent_events.retain(|&t| (now_ms - t) < RATE_WINDOW_MS);

            if recent_events.len() >= RATE_THRESHOLD {
                warn!(
                    "ETW: rapid crypto activity detected — {} ops in {}s from PID {}",
                    recent_events.len(),
                    RATE_WINDOW_MS / 1000,
                    event.process_id
                );

                let alert = CognitiveMessage::SecurityAlert {
                    threat_level: "HIGH".to_string(),
                    details: format!(
                        "Rapid symmetric encryption detected: {} crypto operations in {}s \
                         from PID {}. Possible ransomware activity.",
                        recent_events.len(),
                        RATE_WINDOW_MS / 1000,
                        event.process_id
                    ),
                };

                if self.prime_tx.send(alert).await.is_err() {
                    warn!("ETW dispatch: Prime channel closed");
                    return;
                }

                // Reset after alert to avoid flooding.
                recent_events.clear();
            }
        }
    }
}

impl Drop for EtwMonitor {
    fn drop(&mut self) {
        // Close the trace consumer.
        if let Some(handle) = self.trace_handle.take() {
            unsafe {
                let _ = CloseTrace(handle);
            }
        }

        // Stop the trace session.
        let props_size = mem::size_of::<EVENT_TRACE_PROPERTIES>() + 128;
        let mut buffer = vec![0u8; props_size];
        let props = unsafe { &mut *(buffer.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES) };
        props.Wnode.BufferSize = props_size as u32;

        unsafe {
            let _ = ControlTraceW(
                self.session_handle,
                windows::core::PCWSTR(ptr::null()),
                props,
                EVENT_TRACE_CONTROL_STOP,
            );
        }

        info!("ETW session stopped: {SESSION_NAME}");
    }
}

/// Raw ETW callback — invoked by `ProcessTrace` for each event.
///
/// This is a C-style function pointer, so it cannot capture any state.
/// We use the global `ALERT_SENDER` to push events to the async world.
unsafe extern "system" fn etw_event_callback(event_record: *mut EVENT_RECORD) {
    let record = unsafe { &*event_record };

    let event = EtwCryptoEvent {
        provider_id: record.EventHeader.ProviderId,
        event_id: record.EventHeader.EventDescriptor.Id,
        process_id: record.EventHeader.ProcessId,
        thread_id: record.EventHeader.ThreadId,
        timestamp: record.EventHeader.TimeStamp,
    };

    if let Some(sender) = ALERT_SENDER.get() {
        let _ = sender.send(event);
    }
}
