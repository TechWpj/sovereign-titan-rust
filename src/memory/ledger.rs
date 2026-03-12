//! Encrypted Ledger — AES-256-GCM encrypted JSON state persistence.
//!
//! Ported from `sovereign_titan/memory/ledger.py`. Stores user goals,
//! background tasks, and preferences as encrypted JSON on disk.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aes_gcm::aead::{Aead, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, KeyInit};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Default ledger file location.
const DEFAULT_LEDGER_PATH: &str = "workspace/ledger.enc";

/// Default encryption key file.
const DEFAULT_KEY_PATH: &str = "workspace/.ledger_key";

/// A section in the ledger with a heading and content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerSection {
    pub heading: String,
    pub content: String,
}

/// The serializable ledger state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LedgerState {
    pub sections: Vec<LedgerSection>,
    pub status: String,
    pub goals: Vec<String>,
    pub preferences: HashMap<String, String>,
}

/// Encrypted ledger for persistent state tracking.
pub struct Ledger {
    filepath: PathBuf,
    key_path: PathBuf,
    state: Mutex<LedgerState>,
    cipher_key: [u8; 32],
}

impl Ledger {
    /// Create or load a ledger from the given paths.
    pub fn new(filepath: Option<&str>, key_path: Option<&str>) -> Result<Self> {
        let filepath = PathBuf::from(filepath.unwrap_or(DEFAULT_LEDGER_PATH));
        let key_path = PathBuf::from(key_path.unwrap_or(DEFAULT_KEY_PATH));

        // Ensure parent directories exist.
        if let Some(parent) = filepath.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Some(parent) = key_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Load or generate encryption key.
        let cipher_key = Self::load_or_create_key(&key_path)?;

        // Load existing state if present.
        let state = if filepath.exists() {
            match Self::decrypt_file(&filepath, &cipher_key) {
                Ok(s) => {
                    info!("Ledger: loaded existing state ({} sections)", s.sections.len());
                    s
                }
                Err(e) => {
                    warn!("Ledger: failed to decrypt existing file, starting fresh: {e}");
                    LedgerState::default()
                }
            }
        } else {
            LedgerState::default()
        };

        Ok(Self {
            filepath,
            key_path,
            state: Mutex::new(state),
            cipher_key,
        })
    }

    /// Load an existing key or generate a new one.
    fn load_or_create_key(path: &Path) -> Result<[u8; 32]> {
        if path.exists() {
            let bytes = fs::read(path).context("failed to read ledger key")?;
            if bytes.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                return Ok(key);
            }
            warn!("Ledger: key file has wrong size, regenerating");
        }

        // Generate a new random key.
        let key = Aes256Gcm::generate_key(OsRng);
        let key_bytes: [u8; 32] = key.into();
        fs::write(path, &key_bytes).context("failed to write ledger key")?;
        info!("Ledger: generated new encryption key");
        Ok(key_bytes)
    }

    /// Decrypt the ledger file and deserialize the state.
    fn decrypt_file(path: &Path, key: &[u8; 32]) -> Result<LedgerState> {
        let data = fs::read(path)?;
        if data.len() < 12 {
            anyhow::bail!("encrypted file too short");
        }

        let (nonce_bytes, ciphertext) = data.split_at(12);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
        let nonce = aes_gcm::Nonce::from_slice(nonce_bytes);

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("decryption failed: {e}"))?;

        let state: LedgerState = serde_json::from_slice(&plaintext)?;
        Ok(state)
    }

    /// Encrypt and persist the current state to disk.
    fn persist(&self) -> Result<()> {
        let state = self.state.lock().unwrap();
        let plaintext = serde_json::to_vec_pretty(&*state)?;

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.cipher_key));
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_ref())
            .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;

        // Write nonce + ciphertext atomically via temp file.
        let tmp = self.filepath.with_extension("tmp");
        let mut data = nonce.to_vec();
        data.extend_from_slice(&ciphertext);
        fs::write(&tmp, &data)?;
        fs::rename(&tmp, &self.filepath)?;

        Ok(())
    }

    // ── Public API ───────────────────────────────────────────────────────────

    /// Append or replace a section by heading.
    pub fn set_section(&self, heading: &str, content: &str) -> Result<()> {
        {
            let mut state = self.state.lock().unwrap();
            if let Some(section) = state.sections.iter_mut().find(|s| s.heading == heading) {
                section.content = content.to_string();
            } else {
                state.sections.push(LedgerSection {
                    heading: heading.to_string(),
                    content: content.to_string(),
                });
            }
        }
        self.persist()
    }

    /// Read a section by heading.
    pub fn get_section(&self, heading: &str) -> Option<String> {
        let state = self.state.lock().unwrap();
        let lower = heading.to_lowercase();
        state
            .sections
            .iter()
            .find(|s| s.heading.to_lowercase() == lower)
            .map(|s| s.content.clone())
    }

    /// Set a user preference.
    pub fn set_preference(&self, key: &str, value: &str) -> Result<()> {
        {
            let mut state = self.state.lock().unwrap();
            state.preferences.insert(key.to_string(), value.to_string());
        }
        self.persist()
    }

    /// Get a user preference.
    pub fn get_preference(&self, key: &str) -> Option<String> {
        let state = self.state.lock().unwrap();
        state.preferences.get(key).cloned()
    }

    /// Add a goal.
    pub fn add_goal(&self, goal: &str) -> Result<()> {
        {
            let mut state = self.state.lock().unwrap();
            if !state.goals.iter().any(|g| g == goal) {
                state.goals.push(goal.to_string());
            }
        }
        self.persist()
    }

    /// Get all goals.
    pub fn goals(&self) -> Vec<String> {
        let state = self.state.lock().unwrap();
        state.goals.clone()
    }

    /// Set the status line.
    pub fn set_status(&self, status: &str) -> Result<()> {
        {
            let mut state = self.state.lock().unwrap();
            state.status = status.to_string();
        }
        self.persist()
    }

    /// Get a compact summary for prompt injection.
    pub fn get_summary(&self) -> String {
        let state = self.state.lock().unwrap();
        let mut summary = String::new();

        if !state.goals.is_empty() {
            summary.push_str("Goals: ");
            summary.push_str(&state.goals.join(", "));
            summary.push('\n');
        }

        if !state.preferences.is_empty() {
            summary.push_str("Preferences: ");
            let prefs: Vec<String> = state
                .preferences
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            summary.push_str(&prefs.join(", "));
            summary.push('\n');
        }

        let headings: Vec<&str> = state.sections.iter().map(|s| s.heading.as_str()).collect();
        if !headings.is_empty() {
            summary.push_str("Ledger sections: ");
            summary.push_str(&headings.join(", "));
            summary.push('\n');
        }

        if !state.status.is_empty() {
            summary.push_str("Status: ");
            summary.push_str(&state.status);
        }

        summary
    }

    /// Clear all state and delete the file.
    pub fn clear(&self) -> Result<()> {
        {
            let mut state = self.state.lock().unwrap();
            *state = LedgerState::default();
        }
        if self.filepath.exists() {
            fs::remove_file(&self.filepath)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_ledger() -> Ledger {
        let dir = env::temp_dir().join(format!("titan_ledger_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let fp = dir.join("ledger.enc");
        let kp = dir.join("key");
        Ledger::new(
            Some(fp.to_str().unwrap()),
            Some(kp.to_str().unwrap()),
        )
        .unwrap()
    }

    #[test]
    fn test_section_roundtrip() {
        let ledger = temp_ledger();
        ledger.set_section("Tasks", "Buy groceries").unwrap();
        assert_eq!(ledger.get_section("tasks"), Some("Buy groceries".to_string()));
    }

    #[test]
    fn test_preference_roundtrip() {
        let ledger = temp_ledger();
        ledger.set_preference("theme", "dark").unwrap();
        assert_eq!(ledger.get_preference("theme"), Some("dark".to_string()));
    }

    #[test]
    fn test_persistence_across_loads() {
        let dir = env::temp_dir().join(format!("titan_ledger_persist_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let fp = dir.join("ledger.enc");
        let kp = dir.join("key");

        {
            let ledger = Ledger::new(Some(fp.to_str().unwrap()), Some(kp.to_str().unwrap())).unwrap();
            ledger.add_goal("conquer the world").unwrap();
            ledger.set_section("Notes", "Important stuff").unwrap();
        }

        {
            let ledger = Ledger::new(Some(fp.to_str().unwrap()), Some(kp.to_str().unwrap())).unwrap();
            assert_eq!(ledger.goals(), vec!["conquer the world".to_string()]);
            assert_eq!(ledger.get_section("Notes"), Some("Important stuff".to_string()));
        }
    }

    #[test]
    fn test_clear() {
        let ledger = temp_ledger();
        ledger.add_goal("test").unwrap();
        ledger.clear().unwrap();
        assert!(ledger.goals().is_empty());
    }
}
