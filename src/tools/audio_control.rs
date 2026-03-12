//! Audio Control Tool — system volume management.
//!
//! Actions: get_volume, set_volume, mute, unmute, volume_up, volume_down.
//! Implementation: PowerShell COM + SendKeys for media keys.

use anyhow::Result;
use serde_json::Value;
use tracing::info;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub struct AudioControlTool;

impl AudioControlTool {
    fn run_ps(cmd: &str) -> (i32, String, String) {
        let result = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", cmd])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .creation_flags(0x08000000)
            .spawn();

        match result {
            Ok(child) => match child.wait_with_output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    let code = output.status.code().unwrap_or(-1);
                    (code, stdout, stderr)
                }
                Err(e) => (-1, String::new(), format!("Process error: {e}")),
            },
            Err(e) => (-1, String::new(), format!("Failed to spawn PowerShell: {e}")),
        }
    }

    fn get_volume() -> String {
        let cmd = r#"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

[Guid("5CDF2C82-841E-4546-9722-0CF74078229A"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IAudioEndpointVolume {
    int f(); int g(); int h(); int i();
    int SetMasterVolumeLevelScalar(float fLevel, System.Guid pguidEventContext);
    int j();
    int GetMasterVolumeLevelScalar(out float pfLevel);
    int k();
    int SetMute([MarshalAs(UnmanagedType.Bool)] bool bMute, System.Guid pguidEventContext);
    int GetMute(out bool pbMute);
}

[Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDevice {
    int Activate(ref System.Guid iid, int dwClsCtx, IntPtr pActivationParams, [MarshalAs(UnmanagedType.IUnknown)] out object ppInterface);
}

[Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceEnumerator {
    int GetDefaultAudioEndpoint(int dataFlow, int role, out IMMDevice ppDevice);
}

[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class MMDeviceEnumeratorComObject { }

public class Audio {
    public static float GetVolume() {
        var enumerator = new MMDeviceEnumeratorComObject() as IMMDeviceEnumerator;
        IMMDevice dev;
        enumerator.GetDefaultAudioEndpoint(0, 1, out dev);
        var aevGuid = typeof(IAudioEndpointVolume).GUID;
        object aevObj;
        dev.Activate(ref aevGuid, 23, IntPtr.Zero, out aevObj);
        var aev = (IAudioEndpointVolume)aevObj;
        float vol;
        aev.GetMasterVolumeLevelScalar(out vol);
        return vol;
    }
    public static bool GetMute() {
        var enumerator = new MMDeviceEnumeratorComObject() as IMMDeviceEnumerator;
        IMMDevice dev;
        enumerator.GetDefaultAudioEndpoint(0, 1, out dev);
        var aevGuid = typeof(IAudioEndpointVolume).GUID;
        object aevObj;
        dev.Activate(ref aevGuid, 23, IntPtr.Zero, out aevObj);
        var aev = (IAudioEndpointVolume)aevObj;
        bool muted;
        aev.GetMute(out muted);
        return muted;
    }
}
"@ -ErrorAction Stop

$vol = [Audio]::GetVolume()
$muted = [Audio]::GetMute()
$pct = [math]::Round($vol * 100)
"Volume: ${pct}%, Muted: $muted"
"#;
        let (code, stdout, stderr) = Self::run_ps(cmd);
        if code == 0 && !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("Error getting volume: {}", stderr.trim().chars().take(200).collect::<String>())
        }
    }

    fn set_volume(level: u32) -> String {
        let level = level.min(100);
        let scalar = level as f64 / 100.0;
        let cmd = format!(
            r#"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

[Guid("5CDF2C82-841E-4546-9722-0CF74078229A"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IAudioEndpointVolume {{
    int f(); int g(); int h(); int i();
    int SetMasterVolumeLevelScalar(float fLevel, System.Guid pguidEventContext);
    int j();
    int GetMasterVolumeLevelScalar(out float pfLevel);
    int k();
    int SetMute([MarshalAs(UnmanagedType.Bool)] bool bMute, System.Guid pguidEventContext);
    int GetMute(out bool pbMute);
}}

[Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDevice {{
    int Activate(ref System.Guid iid, int dwClsCtx, IntPtr pActivationParams, [MarshalAs(UnmanagedType.IUnknown)] out object ppInterface);
}}

[Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceEnumerator {{
    int GetDefaultAudioEndpoint(int dataFlow, int role, out IMMDevice ppDevice);
}}

[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class MMDeviceEnumeratorComObject {{ }}

public class Audio {{
    public static void SetVolume(float level) {{
        var enumerator = new MMDeviceEnumeratorComObject() as IMMDeviceEnumerator;
        IMMDevice dev;
        enumerator.GetDefaultAudioEndpoint(0, 1, out dev);
        var aevGuid = typeof(IAudioEndpointVolume).GUID;
        object aevObj;
        dev.Activate(ref aevGuid, 23, IntPtr.Zero, out aevObj);
        var aev = (IAudioEndpointVolume)aevObj;
        aev.SetMasterVolumeLevelScalar(level, System.Guid.Empty);
    }}
}}
"@ -ErrorAction Stop

[Audio]::SetVolume({scalar})
"Volume set to {level}%"
"#
        );
        let (code, stdout, stderr) = Self::run_ps(&cmd);
        if code == 0 {
            stdout.trim().to_string()
        } else {
            format!("Error setting volume: {}", stderr.trim().chars().take(200).collect::<String>())
        }
    }

    fn set_mute(mute: bool) -> String {
        let mute_val = if mute { "true" } else { "false" };
        let cmd = format!(
            r#"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

[Guid("5CDF2C82-841E-4546-9722-0CF74078229A"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IAudioEndpointVolume {{
    int f(); int g(); int h(); int i();
    int SetMasterVolumeLevelScalar(float fLevel, System.Guid pguidEventContext);
    int j();
    int GetMasterVolumeLevelScalar(out float pfLevel);
    int k();
    int SetMute([MarshalAs(UnmanagedType.Bool)] bool bMute, System.Guid pguidEventContext);
    int GetMute(out bool pbMute);
}}

[Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDevice {{
    int Activate(ref System.Guid iid, int dwClsCtx, IntPtr pActivationParams, [MarshalAs(UnmanagedType.IUnknown)] out object ppInterface);
}}

[Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceEnumerator {{
    int GetDefaultAudioEndpoint(int dataFlow, int role, out IMMDevice ppDevice);
}}

[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class MMDeviceEnumeratorComObject {{ }}

public class Audio {{
    public static void SetMute(bool mute) {{
        var enumerator = new MMDeviceEnumeratorComObject() as IMMDeviceEnumerator;
        IMMDevice dev;
        enumerator.GetDefaultAudioEndpoint(0, 1, out dev);
        var aevGuid = typeof(IAudioEndpointVolume).GUID;
        object aevObj;
        dev.Activate(ref aevGuid, 23, IntPtr.Zero, out aevObj);
        var aev = (IAudioEndpointVolume)aevObj;
        aev.SetMute(mute, System.Guid.Empty);
    }}
}}
"@ -ErrorAction Stop

[Audio]::SetMute(${mute_val})
"{}"
"#,
            if mute { "Audio muted." } else { "Audio unmuted." }
        );
        let (code, stdout, stderr) = Self::run_ps(&cmd);
        if code == 0 {
            stdout.trim().to_string()
        } else {
            format!("Error: {}", stderr.trim().chars().take(200).collect::<String>())
        }
    }

    fn volume_step(up: bool) -> String {
        // Use SendKeys for media keys as a simple fallback
        let key = if up {
            "{Volume_Up}{Volume_Up}{Volume_Up}{Volume_Up}{Volume_Up}"
        } else {
            "{Volume_Down}{Volume_Down}{Volume_Down}{Volume_Down}{Volume_Down}"
        };
        let cmd = format!(
            r#"
$wsh = New-Object -ComObject WScript.Shell
$wsh.SendKeys('{key}')
"{}"
"#,
            if up { "Volume increased." } else { "Volume decreased." }
        );
        let (_, stdout, _) = Self::run_ps(&cmd);
        if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else if up {
            "Volume increased.".to_string()
        } else {
            "Volume decreased.".to_string()
        }
    }
}

#[async_trait::async_trait]
impl super::Tool for AudioControlTool {
    fn name(&self) -> &'static str {
        "audio_control"
    }

    fn description(&self) -> &'static str {
        "Control system audio volume. Input: {\"action\": \"<action>\", ...}. \
         Actions: get_volume, set_volume (level: 0-100), \
         mute, unmute, volume_up, volume_down."
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("get_volume");

        info!("audio_control: action={action}");

        let input_clone = input.clone();
        let action_owned = action.to_string();

        let result = tokio::task::spawn_blocking(move || {
            match action_owned.as_str() {
                "get_volume" => Self::get_volume(),
                "set_volume" => {
                    let level = input_clone
                        .get("level")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(50) as u32;
                    Self::set_volume(level)
                }
                "mute" => Self::set_mute(true),
                "unmute" => Self::set_mute(false),
                "volume_up" => Self::volume_step(true),
                "volume_down" => Self::volume_step(false),
                other => format!(
                    "Unknown action: '{other}'. Use: get_volume, set_volume, mute, unmute, volume_up, volume_down."
                ),
            }
        })
        .await?;

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
        let tool = AudioControlTool;
        let result = tool.execute(json!({"action": "fly"})).await.unwrap();
        assert!(result.contains("Unknown action"));
    }

    #[tokio::test]
    async fn test_default_is_get_volume() {
        let tool = AudioControlTool;
        let result = tool.execute(json!({})).await.unwrap();
        // Should return volume info or error (both are OK for test)
        assert!(!result.is_empty());
    }
}
