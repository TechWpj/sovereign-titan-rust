//! AppDiscovery — Windows application discovery with 5-tier fuzzy resolution.
//!
//! Ported from `sovereign_titan/routing/app_discovery.py`. Scans multiple
//! sources (Start Apps, Start Menu, Registry, PATH, common paths) to build
//! a cache of installed applications and resolves user queries through
//! exact match, aliases, name variants, substring, and fuzzy matching.

use std::collections::HashMap;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use tracing::{info, warn};

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

/// Source from which an app was discovered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppSource {
    WellKnown,
    StartApps,
    StartMenu,
    Registry,
    PathLookup,
    CommonPaths,
}

/// A discovered application entry.
#[derive(Debug, Clone)]
pub struct AppEntry {
    pub name: String,
    pub exe_path: String,
    pub source: AppSource,
}

/// Resolution result with match quality info.
#[derive(Debug, Clone)]
pub struct ResolveResult {
    pub entry: AppEntry,
    pub tier: ResolveTier,
    pub score: f64,
}

/// Which resolution tier matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveTier {
    Exact,
    Alias,
    NameVariant,
    Substring,
    Fuzzy,
}

impl std::fmt::Display for ResolveTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveTier::Exact => write!(f, "exact"),
            ResolveTier::Alias => write!(f, "alias"),
            ResolveTier::NameVariant => write!(f, "name_variant"),
            ResolveTier::Substring => write!(f, "substring"),
            ResolveTier::Fuzzy => write!(f, "fuzzy"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Well-known apps and aliases
// ─────────────────────────────────────────────────────────────────────────────

/// Well-known apps with guaranteed paths on typical Windows installs.
const WELL_KNOWN_APPS: &[(&str, &str)] = &[
    ("notepad", "notepad.exe"),
    ("calculator", "calc.exe"),
    ("paint", "mspaint.exe"),
    ("task manager", "taskmgr.exe"),
    ("command prompt", "cmd.exe"),
    ("powershell", "powershell.exe"),
    ("windows terminal", "wt.exe"),
    ("file explorer", "explorer.exe"),
    ("snipping tool", "snippingtool.exe"),
    ("control panel", "control.exe"),
    ("wordpad", "write.exe"),
    ("character map", "charmap.exe"),
    ("disk management", "diskmgmt.msc"),
    ("device manager", "devmgmt.msc"),
    ("event viewer", "eventvwr.msc"),
    ("registry editor", "regedit.exe"),
    ("remote desktop", "mstsc.exe"),
    // ── Browsers ──
    ("google chrome", r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
    ("mozilla firefox", r"C:\Program Files\Mozilla Firefox\firefox.exe"),
    ("microsoft edge", r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
    ("brave", r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe"),
    // ── Common third-party ──
    ("discord", r"C:\Users\treyd\AppData\Local\Discord\Update.exe --processStart Discord.exe"),
    ("spotify", r"C:\Users\treyd\AppData\Roaming\Spotify\Spotify.exe"),
    ("steam", r"C:\Program Files (x86)\Steam\steam.exe"),
    ("vlc", r"C:\Program Files\VideoLAN\VLC\vlc.exe"),
    ("obs studio", r"C:\Program Files\obs-studio\bin\64bit\obs64.exe"),
    ("obs", r"C:\Program Files\obs-studio\bin\64bit\obs64.exe"),
    ("visual studio code", r"C:\Users\treyd\AppData\Local\Programs\Microsoft VS Code\Code.exe"),
    ("gimp", r"C:\Program Files\GIMP 2\bin\gimp-2.10.exe"),
    ("7-zip", r"C:\Program Files\7-Zip\7zFM.exe"),
    ("winrar", r"C:\Program Files\WinRAR\WinRAR.exe"),
    ("audacity", r"C:\Program Files\Audacity\audacity.exe"),
];

/// Alias → canonical name mappings (62+ aliases).
const ALIASES: &[(&str, &str)] = &[
    // Browsers
    ("chrome", "google chrome"),
    ("firefox", "mozilla firefox"),
    ("edge", "microsoft edge"),
    ("msedge", "microsoft edge"),
    // Dev tools
    ("vscode", "visual studio code"),
    ("vs code", "visual studio code"),
    ("code", "visual studio code"),
    // Media
    ("vlc media player", "vlc"),
    ("vlc player", "vlc"),
    // System
    ("calc", "calculator"),
    ("cmd", "command prompt"),
    ("terminal", "windows terminal"),
    ("wt", "windows terminal"),
    ("explorer", "file explorer"),
    ("taskmgr", "task manager"),
    ("regedit", "registry editor"),
    ("mstsc", "remote desktop"),
    ("rdp", "remote desktop"),
    ("devmgmt", "device manager"),
    ("eventvwr", "event viewer"),
    // Messaging
    ("dc", "discord"),
    // Entertainment
    ("obs studio", "obs"),
    // Archive
    ("7z", "7-zip"),
    ("7zip", "7-zip"),
    // Creative
    ("gimp2", "gimp"),
    ("the gimp", "gimp"),
    // Gaming
    ("steam client", "steam"),
    // Music
    ("spotify music", "spotify"),
    // Office
    ("word", "microsoft word"),
    ("excel", "microsoft excel"),
    ("powerpoint", "microsoft powerpoint"),
    ("ppt", "microsoft powerpoint"),
    ("outlook", "microsoft outlook"),
    // Misc
    ("notepad++", "notepad++"),
    ("npp", "notepad++"),
    ("paint.net", "paint.net"),
    ("pdn", "paint.net"),
    // More system aliases
    ("settings", "windows settings"),
    ("control", "control panel"),
    ("snip", "snipping tool"),
    ("screenshot", "snipping tool"),
    ("charmap", "character map"),
    // Browsers alternate
    ("google", "google chrome"),
    ("brave browser", "brave"),
    // Dev
    ("git bash", "git bash"),
    ("python", "python"),
    ("py", "python"),
    ("node", "node"),
    ("nodejs", "node"),
    ("npm", "npm"),
    ("cargo", "cargo"),
    ("rustc", "rustc"),
    // Media
    ("audacity audio", "audacity"),
    // Gaming
    ("epic games", "epic games launcher"),
    ("epic", "epic games launcher"),
    // Comms
    ("slack", "slack"),
    ("teams", "microsoft teams"),
    ("ms teams", "microsoft teams"),
    ("zoom", "zoom"),
    ("telegram", "telegram"),
    ("whatsapp", "whatsapp"),
    ("signal", "signal"),
];

/// Words to strip when building name variants.
const STRIP_WORDS: &[&str] = &[
    "the", "a", "an", "app", "application", "program", "software",
    "tool", "utility", "client", "desktop", "portable",
];

// ─────────────────────────────────────────────────────────────────────────────
// AppDiscovery
// ─────────────────────────────────────────────────────────────────────────────

/// Windows application discovery engine.
pub struct AppDiscovery {
    /// Canonical name (lowercase) → AppEntry.
    cache: HashMap<String, AppEntry>,
    /// Alias (lowercase) → canonical name (lowercase).
    aliases: HashMap<String, String>,
    /// Timestamp of last full scan.
    last_scan: Option<Instant>,
    /// Cache TTL.
    cache_ttl: Duration,
}

impl AppDiscovery {
    /// Create a new AppDiscovery with default settings.
    pub fn new() -> Self {
        let mut aliases = HashMap::new();
        for &(alias, canonical) in ALIASES {
            aliases.insert(alias.to_lowercase(), canonical.to_lowercase());
        }

        Self {
            cache: HashMap::new(),
            aliases,
            last_scan: None,
            cache_ttl: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Number of cached apps.
    pub fn app_count(&self) -> usize {
        self.cache.len()
    }

    /// Whether the cache needs refreshing.
    pub fn needs_scan(&self) -> bool {
        self.last_scan
            .map(|t| t.elapsed() > self.cache_ttl)
            .unwrap_or(true)
    }

    /// Get a summary of discovered apps for prompt injection.
    pub fn summary(&self) -> String {
        if self.cache.is_empty() {
            return String::from("No apps discovered yet.");
        }
        let mut names: Vec<&str> = self.cache.values().map(|e| e.name.as_str()).collect();
        names.sort();
        names.truncate(50);
        format!(
            "{} apps discovered. Sample: {}",
            self.cache.len(),
            names.join(", ")
        )
    }

    /// Full scan: populate cache from all sources.
    pub fn scan(&mut self) {
        let start = Instant::now();
        info!("AppDiscovery: starting full scan...");

        // 1. Well-known apps (always available)
        self.load_well_known();

        // 2. Start Apps (Get-StartApps)
        self.scan_start_apps();

        // 3. Start Menu .lnk files
        self.scan_start_menu();

        // 4. Registry uninstall keys
        self.scan_registry();

        // 5. PATH executables
        self.scan_path();

        // 6. Common installation paths
        self.scan_common_paths();

        self.last_scan = Some(Instant::now());
        info!(
            "AppDiscovery: scan complete — {} apps in {:?}",
            self.cache.len(),
            start.elapsed()
        );
    }

    fn load_well_known(&mut self) {
        for &(name, path) in WELL_KNOWN_APPS {
            self.cache.insert(
                name.to_lowercase(),
                AppEntry {
                    name: name.to_string(),
                    exe_path: path.to_string(),
                    source: AppSource::WellKnown,
                },
            );
        }
    }

    fn scan_start_apps(&mut self) {
        let cmd = "Get-StartApps | Select-Object Name, AppID | ConvertTo-Json -Compress";
        let output = run_ps_silent(cmd);
        if output.is_empty() {
            return;
        }

        // Parse JSON array or single object.
        if let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&output) {
            for entry in entries {
                if let (Some(name), Some(app_id)) = (
                    entry.get("Name").and_then(|v| v.as_str()),
                    entry.get("AppID").and_then(|v| v.as_str()),
                ) {
                    let key = name.to_lowercase();
                    if !self.cache.contains_key(&key) {
                        self.cache.insert(
                            key,
                            AppEntry {
                                name: name.to_string(),
                                exe_path: app_id.to_string(),
                                source: AppSource::StartApps,
                            },
                        );
                    }
                }
            }
        }
    }

    fn scan_start_menu(&mut self) {
        // Walk common Start Menu locations for .lnk files
        let ps = r#"
$paths = @(
    "$env:ProgramData\Microsoft\Windows\Start Menu\Programs",
    "$env:APPDATA\Microsoft\Windows\Start Menu\Programs"
)
$shell = New-Object -ComObject WScript.Shell
$results = @()
foreach ($root in $paths) {
    if (Test-Path $root) {
        Get-ChildItem -Path $root -Filter '*.lnk' -Recurse -ErrorAction SilentlyContinue |
            ForEach-Object {
                try {
                    $lnk = $shell.CreateShortcut($_.FullName)
                    $results += @{ Name = $_.BaseName; Path = $lnk.TargetPath }
                } catch {}
            }
    }
}
$results | ConvertTo-Json -Compress
"#;

        let output = run_ps_silent(ps);
        if output.is_empty() {
            return;
        }

        if let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&output) {
            for entry in entries {
                if let (Some(name), Some(path)) = (
                    entry.get("Name").and_then(|v| v.as_str()),
                    entry.get("Path").and_then(|v| v.as_str()),
                ) {
                    if path.is_empty() {
                        continue;
                    }
                    let key = name.to_lowercase();
                    if !self.cache.contains_key(&key) {
                        self.cache.insert(
                            key,
                            AppEntry {
                                name: name.to_string(),
                                exe_path: path.to_string(),
                                source: AppSource::StartMenu,
                            },
                        );
                    }
                }
            }
        }
    }

    fn scan_registry(&mut self) {
        let ps = r#"
$keys = @(
    'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'
)
$results = @()
foreach ($key in $keys) {
    Get-ItemProperty $key -ErrorAction SilentlyContinue |
        Where-Object { $_.DisplayName -and ($_.InstallLocation -or $_.DisplayIcon) } |
        ForEach-Object {
            $exe = if ($_.InstallLocation) {
                $exeFiles = Get-ChildItem -Path $_.InstallLocation -Filter '*.exe' -Depth 1 -ErrorAction SilentlyContinue | Select-Object -First 1
                if ($exeFiles) { $exeFiles.FullName } else { $_.InstallLocation }
            } elseif ($_.DisplayIcon) {
                ($_.DisplayIcon -split ',')[0]
            } else { '' }
            if ($exe) {
                $results += @{ Name = $_.DisplayName; Path = $exe }
            }
        }
}
$results | ConvertTo-Json -Compress
"#;

        let output = run_ps_silent(ps);
        if output.is_empty() {
            return;
        }

        if let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&output) {
            for entry in entries {
                if let (Some(name), Some(path)) = (
                    entry.get("Name").and_then(|v| v.as_str()),
                    entry.get("Path").and_then(|v| v.as_str()),
                ) {
                    if path.is_empty() {
                        continue;
                    }
                    let key = name.to_lowercase();
                    if !self.cache.contains_key(&key) {
                        self.cache.insert(
                            key,
                            AppEntry {
                                name: name.to_string(),
                                exe_path: path.to_string(),
                                source: AppSource::Registry,
                            },
                        );
                    }
                }
            }
        }
    }

    fn scan_path(&mut self) {
        if let Ok(path_var) = std::env::var("PATH") {
            for dir in path_var.split(';') {
                let dir_path = std::path::Path::new(dir);
                if !dir_path.is_dir() {
                    continue;
                }
                if let Ok(entries) = std::fs::read_dir(dir_path) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().is_some_and(|ext| ext == "exe") {
                            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                                let key = stem.to_lowercase();
                                if !self.cache.contains_key(&key) {
                                    self.cache.insert(
                                        key,
                                        AppEntry {
                                            name: stem.to_string(),
                                            exe_path: path.to_string_lossy().to_string(),
                                            source: AppSource::PathLookup,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn scan_common_paths(&mut self) {
        let common_dirs = [
            r"C:\Program Files",
            r"C:\Program Files (x86)",
        ];

        // Also check %LOCALAPPDATA%\Programs
        let mut dirs: Vec<String> = common_dirs.iter().map(|s| s.to_string()).collect();
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(format!(r"{local}\Programs"));
        }

        for dir in &dirs {
            let dir_path = std::path::Path::new(dir);
            if !dir_path.is_dir() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(dir_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    // Look for .exe in top-level and one level down
                    let folder_name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();

                    if let Some(exe) = find_exe_in_dir(&path, 1) {
                        let key = folder_name.to_lowercase();
                        if !self.cache.contains_key(&key) {
                            self.cache.insert(
                                key,
                                AppEntry {
                                    name: folder_name,
                                    exe_path: exe,
                                    source: AppSource::CommonPaths,
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    // ── Resolution (5-tier) ─────────────────────────────────────────────

    /// Resolve a user query to an application.
    ///
    /// Resolution tiers:
    /// 1. Exact match (HashMap lookup)
    /// 2. Alias → canonical → lookup
    /// 3. Name variants (strip articles/suffixes)
    /// 4. Substring contains (length ratio >= 0.8)
    /// 5. Fuzzy via Jaro-Winkler (cutoff 0.85)
    pub fn resolve(&self, query: &str) -> Option<ResolveResult> {
        let q = query.trim().to_lowercase();

        if q.is_empty() {
            return None;
        }

        // Tier 1: Exact match
        if let Some(entry) = self.cache.get(&q) {
            return Some(ResolveResult {
                entry: entry.clone(),
                tier: ResolveTier::Exact,
                score: 1.0,
            });
        }

        // Tier 2: Alias → canonical → lookup
        if let Some(canonical) = self.aliases.get(&q) {
            if let Some(entry) = self.cache.get(canonical.as_str()) {
                return Some(ResolveResult {
                    entry: entry.clone(),
                    tier: ResolveTier::Alias,
                    score: 1.0,
                });
            }
        }

        // Tier 3: Name variants (strip articles and suffixes)
        let variants = build_name_variants(&q);
        for variant in &variants {
            if let Some(entry) = self.cache.get(variant.as_str()) {
                return Some(ResolveResult {
                    entry: entry.clone(),
                    tier: ResolveTier::NameVariant,
                    score: 0.95,
                });
            }
            // Also check aliases for variants
            if let Some(canonical) = self.aliases.get(variant.as_str()) {
                if let Some(entry) = self.cache.get(canonical.as_str()) {
                    return Some(ResolveResult {
                        entry: entry.clone(),
                        tier: ResolveTier::NameVariant,
                        score: 0.95,
                    });
                }
            }
        }

        // Tier 4: Substring match with length ratio >= 0.8
        let mut best_substring: Option<(&str, &AppEntry, f64)> = None;
        for (key, entry) in &self.cache {
            if key.contains(&q) || q.contains(key.as_str()) {
                let ratio = q.len().min(key.len()) as f64 / q.len().max(key.len()) as f64;
                if ratio >= 0.8 {
                    if best_substring.as_ref().map_or(true, |b| ratio > b.2) {
                        best_substring = Some((key, entry, ratio));
                    }
                }
            }
        }
        if let Some((_, entry, score)) = best_substring {
            return Some(ResolveResult {
                entry: entry.clone(),
                tier: ResolveTier::Substring,
                score,
            });
        }

        // Tier 5: Fuzzy match via Jaro-Winkler (cutoff 0.85)
        let mut best_fuzzy: Option<(&AppEntry, f64)> = None;
        for (key, entry) in &self.cache {
            let score = strsim::jaro_winkler(&q, key);
            if score >= 0.85 {
                if best_fuzzy.as_ref().map_or(true, |b| score > b.1) {
                    best_fuzzy = Some((entry, score));
                }
            }
        }
        if let Some((entry, score)) = best_fuzzy {
            return Some(ResolveResult {
                entry: entry.clone(),
                tier: ResolveTier::Fuzzy,
                score,
            });
        }

        None
    }

    /// Quick resolve that just returns the exe path (for fast launch).
    pub fn resolve_exe(&self, query: &str) -> Option<String> {
        self.resolve(query).map(|r| r.entry.exe_path)
    }

    /// Check if a query can be resolved.
    pub fn can_resolve(&self, query: &str) -> bool {
        self.resolve(query).is_some()
    }

    /// List all cached app names (sorted).
    pub fn list_apps(&self) -> Vec<String> {
        let mut names: Vec<String> = self.cache.values().map(|e| e.name.clone()).collect();
        names.sort();
        names.dedup();
        names
    }
}

impl Default for AppDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Run a PowerShell command silently and return stdout.
fn run_ps_silent(cmd: &str) -> String {
    let result = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", cmd])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .spawn();

    match result {
        Ok(child) => match child.wait_with_output() {
            Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
            Err(e) => {
                warn!("AppDiscovery PS command failed: {e}");
                String::new()
            }
        },
        Err(e) => {
            warn!("AppDiscovery: failed to spawn PowerShell: {e}");
            String::new()
        }
    }
}

/// Find an .exe file within a directory (up to `depth` levels).
fn find_exe_in_dir(dir: &std::path::Path, depth: usize) -> Option<String> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "exe") {
                return Some(path.to_string_lossy().to_string());
            }
        }
    }

    if depth > 0 {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(exe) = find_exe_in_dir(&path, depth - 1) {
                        return Some(exe);
                    }
                }
            }
        }
    }

    None
}

/// Build name variants by stripping common words.
fn build_name_variants(query: &str) -> Vec<String> {
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut variants = Vec::new();

    // Strip each stop word
    for stop in STRIP_WORDS {
        let filtered: Vec<&str> = words.iter().filter(|w| **w != *stop).copied().collect();
        if filtered.len() != words.len() && !filtered.is_empty() {
            variants.push(filtered.join(" "));
        }
    }

    // Also try without all stop words at once
    let all_filtered: Vec<&str> = words
        .iter()
        .filter(|w| !STRIP_WORDS.contains(w))
        .copied()
        .collect();
    if all_filtered.len() != words.len() && !all_filtered.is_empty() {
        let joined = all_filtered.join(" ");
        if !variants.contains(&joined) {
            variants.push(joined);
        }
    }

    variants
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a test discovery with well-known apps only (no PS scan).
    fn test_discovery() -> AppDiscovery {
        let mut d = AppDiscovery::new();
        d.load_well_known();
        d.last_scan = Some(Instant::now());
        d
    }

    #[test]
    fn test_new_has_aliases() {
        let d = AppDiscovery::new();
        assert!(d.aliases.contains_key("chrome"));
        assert!(d.aliases.contains_key("vscode"));
        assert!(d.aliases.contains_key("calc"));
    }

    #[test]
    fn test_load_well_known() {
        let d = test_discovery();
        assert!(d.cache.contains_key("notepad"));
        assert!(d.cache.contains_key("google chrome"));
        assert!(d.cache.contains_key("discord"));
        assert!(d.app_count() > 20);
    }

    #[test]
    fn test_resolve_exact() {
        let d = test_discovery();
        let r = d.resolve("notepad").unwrap();
        assert_eq!(r.tier, ResolveTier::Exact);
        assert_eq!(r.entry.exe_path, "notepad.exe");
        assert_eq!(r.score, 1.0);
    }

    #[test]
    fn test_resolve_exact_case_insensitive() {
        let d = test_discovery();
        let r = d.resolve("NotePad").unwrap();
        assert_eq!(r.tier, ResolveTier::Exact);
    }

    #[test]
    fn test_resolve_alias_chrome() {
        let d = test_discovery();
        let r = d.resolve("chrome").unwrap();
        assert_eq!(r.tier, ResolveTier::Alias);
        assert!(r.entry.exe_path.contains("chrome"));
    }

    #[test]
    fn test_resolve_alias_vscode() {
        let d = test_discovery();
        let r = d.resolve("vscode").unwrap();
        assert_eq!(r.tier, ResolveTier::Alias);
        assert!(r.entry.exe_path.contains("Code"));
    }

    #[test]
    fn test_resolve_alias_calc() {
        let d = test_discovery();
        let r = d.resolve("calc").unwrap();
        assert_eq!(r.tier, ResolveTier::Alias);
        assert_eq!(r.entry.exe_path, "calc.exe");
    }

    #[test]
    fn test_resolve_alias_terminal() {
        let d = test_discovery();
        let r = d.resolve("terminal").unwrap();
        assert_eq!(r.tier, ResolveTier::Alias);
        assert!(r.entry.exe_path.contains("wt"));
    }

    #[test]
    fn test_resolve_alias_edge() {
        let d = test_discovery();
        let r = d.resolve("edge").unwrap();
        assert_eq!(r.tier, ResolveTier::Alias);
        assert!(r.entry.exe_path.contains("Edge") || r.entry.exe_path.contains("msedge"));
    }

    #[test]
    fn test_resolve_name_variant() {
        let mut d = test_discovery();
        // Insert something that can be found via variant
        d.cache.insert(
            "music player".to_string(),
            AppEntry {
                name: "Music Player".to_string(),
                exe_path: "musicplayer.exe".to_string(),
                source: AppSource::StartMenu,
            },
        );
        // "the music player" → strip "the" → "music player"
        let r = d.resolve("the music player").unwrap();
        assert_eq!(r.tier, ResolveTier::NameVariant);
    }

    #[test]
    fn test_resolve_substring() {
        let mut d = test_discovery();
        // Insert "myeditor pro" (12 chars), resolve "myeditor" (8 chars)
        // "myeditor" is a substring of "myeditor pro", ratio = 8/12 = 0.67 < 0.8
        // Insert "myeditor+" (9 chars), resolve "myeditor" (8 chars) = 8/9 = 0.89 >= 0.8
        d.cache.insert(
            "myeditor+".to_string(),
            AppEntry {
                name: "MyEditor+".to_string(),
                exe_path: "myeditor.exe".to_string(),
                source: AppSource::CommonPaths,
            },
        );
        let r = d.resolve("myeditor").unwrap();
        assert!(matches!(r.tier, ResolveTier::Substring));
    }

    #[test]
    fn test_resolve_fuzzy() {
        let mut d = test_discovery();
        d.cache.insert(
            "visual studio code".to_string(),
            AppEntry {
                name: "Visual Studio Code".to_string(),
                exe_path: "code.exe".to_string(),
                source: AppSource::WellKnown,
            },
        );
        // "visual studio cod" is close to "visual studio code"
        let r = d.resolve("visual studio cod");
        assert!(r.is_some());
        if let Some(result) = r {
            assert!(matches!(result.tier, ResolveTier::Fuzzy | ResolveTier::Substring));
            assert!(result.score >= 0.85);
        }
    }

    #[test]
    fn test_resolve_nonexistent_returns_none() {
        let d = test_discovery();
        assert!(d.resolve("xyznonexistent123").is_none());
    }

    #[test]
    fn test_resolve_empty_returns_none() {
        let d = test_discovery();
        assert!(d.resolve("").is_none());
    }

    #[test]
    fn test_resolve_exe() {
        let d = test_discovery();
        assert_eq!(d.resolve_exe("notepad"), Some("notepad.exe".to_string()));
    }

    #[test]
    fn test_can_resolve() {
        let d = test_discovery();
        assert!(d.can_resolve("notepad"));
        assert!(d.can_resolve("chrome"));
        assert!(!d.can_resolve("xyznonexistent"));
    }

    #[test]
    fn test_list_apps() {
        let d = test_discovery();
        let apps = d.list_apps();
        assert!(!apps.is_empty());
        // Verify sorted
        for i in 1..apps.len() {
            assert!(apps[i - 1] <= apps[i], "list_apps should be sorted");
        }
    }

    #[test]
    fn test_summary() {
        let d = test_discovery();
        let s = d.summary();
        assert!(s.contains("apps discovered"));
    }

    #[test]
    fn test_needs_scan_initially() {
        let d = AppDiscovery::new();
        assert!(d.needs_scan());
    }

    #[test]
    fn test_needs_scan_after_scan() {
        let d = test_discovery();
        assert!(!d.needs_scan());
    }

    #[test]
    fn test_build_name_variants() {
        let variants = build_name_variants("the music app");
        assert!(variants.contains(&"music app".to_string()));
        assert!(variants.contains(&"the music".to_string()));
        assert!(variants.contains(&"music".to_string()));
    }

    #[test]
    fn test_build_name_variants_no_stop_words() {
        let variants = build_name_variants("discord");
        assert!(variants.is_empty());
    }

    #[test]
    fn test_app_source_well_known() {
        let d = test_discovery();
        let r = d.resolve("notepad").unwrap();
        assert_eq!(r.entry.source, AppSource::WellKnown);
    }

    #[test]
    fn test_resolve_tier_display() {
        assert_eq!(format!("{}", ResolveTier::Exact), "exact");
        assert_eq!(format!("{}", ResolveTier::Alias), "alias");
        assert_eq!(format!("{}", ResolveTier::Fuzzy), "fuzzy");
    }

    #[test]
    fn test_discord_resolve() {
        let d = test_discovery();
        let r = d.resolve("discord").unwrap();
        assert_eq!(r.tier, ResolveTier::Exact);
        assert!(r.entry.exe_path.contains("Discord"));
    }

    #[test]
    fn test_dc_alias_to_discord() {
        let d = test_discovery();
        let r = d.resolve("dc").unwrap();
        assert_eq!(r.tier, ResolveTier::Alias);
        assert!(r.entry.exe_path.contains("Discord"));
    }

    #[test]
    fn test_spotify_resolve() {
        let d = test_discovery();
        let r = d.resolve("spotify").unwrap();
        assert_eq!(r.tier, ResolveTier::Exact);
        assert!(r.entry.exe_path.contains("Spotify"));
    }
}
