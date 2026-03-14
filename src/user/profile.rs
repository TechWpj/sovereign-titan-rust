//! User Profile Manager — encrypted user profile store with credential lookup.
//!
//! Ported from `sovereign_titan/user/profile.py`. Stores user identity,
//! address, and per-site account credentials. In this port, profiles are
//! persisted as JSON with a format marker; actual AES-GCM encryption would
//! use the `aes-gcm` crate (already a dependency).

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Physical mailing address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Address {
    pub street: String,
    pub city: String,
    pub state: String,
    pub zip_code: String,
}

impl Default for Address {
    fn default() -> Self {
        Self {
            street: String::new(),
            city: String::new(),
            state: String::new(),
            zip_code: String::new(),
        }
    }
}

/// A stored account credential for a website or service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountEntry {
    pub site: String,
    pub username: String,
    pub email: String,
}

/// The full user profile, including PII and account credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub name: String,
    pub email: String,
    pub phone: String,
    pub date_of_birth: String,
    pub address: Address,
    pub accounts: Vec<AccountEntry>,
    pub google_account: String,
    pub system_pin: String,
}

impl Default for UserProfile {
    fn default() -> Self {
        Self {
            name: String::new(),
            email: String::new(),
            phone: String::new(),
            date_of_birth: String::new(),
            address: Address::default(),
            google_account: String::new(),
            system_pin: String::new(),
            accounts: Vec::new(),
        }
    }
}

/// Wrapper envelope for on-disk profile storage.
///
/// In the future, the `data` field would hold an encrypted blob. For now
/// it stores the JSON-serialized profile directly.
#[derive(Debug, Serialize, Deserialize)]
struct ProfileEnvelope {
    /// Format version marker.
    version: u32,
    /// Whether the payload is encrypted (always `false` in this stub).
    encrypted: bool,
    /// The serialized profile payload.
    data: String,
}

/// Manages loading, saving, and querying a user profile.
pub struct UserProfileManager {
    /// File path for persistence.
    path: String,
    /// The in-memory profile.
    profile: UserProfile,
}

impl UserProfileManager {
    /// Create a new manager, optionally loading an existing profile from disk.
    pub fn new(persist_path: &str) -> Self {
        let mut mgr = Self {
            path: persist_path.to_string(),
            profile: UserProfile::default(),
        };

        if Path::new(persist_path).exists() {
            if let Err(e) = mgr.load() {
                tracing::warn!("UserProfileManager: failed to load profile: {e}");
            }
        }

        mgr
    }

    /// Return a clone of the current profile.
    pub fn get_profile(&self) -> UserProfile {
        self.profile.clone()
    }

    /// Update the profile from a JSON value containing partial fields.
    ///
    /// Merges the provided JSON object into the existing profile. Only
    /// top-level string fields and the `address` sub-object are supported.
    pub fn update_profile(&mut self, updates: serde_json::Value) -> UserProfile {
        if let Some(obj) = updates.as_object() {
            if let Some(v) = obj.get("name").and_then(|v| v.as_str()) {
                self.profile.name = v.to_string();
            }
            if let Some(v) = obj.get("email").and_then(|v| v.as_str()) {
                self.profile.email = v.to_string();
            }
            if let Some(v) = obj.get("phone").and_then(|v| v.as_str()) {
                self.profile.phone = v.to_string();
            }
            if let Some(v) = obj.get("date_of_birth").and_then(|v| v.as_str()) {
                self.profile.date_of_birth = v.to_string();
            }
            if let Some(v) = obj.get("google_account").and_then(|v| v.as_str()) {
                self.profile.google_account = v.to_string();
            }
            if let Some(v) = obj.get("system_pin").and_then(|v| v.as_str()) {
                self.profile.system_pin = v.to_string();
            }
            if let Some(addr) = obj.get("address").and_then(|v| v.as_object()) {
                if let Some(v) = addr.get("street").and_then(|v| v.as_str()) {
                    self.profile.address.street = v.to_string();
                }
                if let Some(v) = addr.get("city").and_then(|v| v.as_str()) {
                    self.profile.address.city = v.to_string();
                }
                if let Some(v) = addr.get("state").and_then(|v| v.as_str()) {
                    self.profile.address.state = v.to_string();
                }
                if let Some(v) = addr.get("zip_code").and_then(|v| v.as_str()) {
                    self.profile.address.zip_code = v.to_string();
                }
            }
        }
        self.profile.clone()
    }

    /// Look up stored credentials for a site (case-insensitive match).
    pub fn get_credentials_for_site(&self, site: &str) -> Option<&AccountEntry> {
        let lower = site.to_lowercase();
        self.profile
            .accounts
            .iter()
            .find(|a| a.site.to_lowercase() == lower)
    }

    /// Add a new account entry or update an existing one for the given site.
    pub fn add_or_update_account(&mut self, site: &str, username: &str, email: &str) {
        let lower = site.to_lowercase();
        if let Some(existing) = self
            .profile
            .accounts
            .iter_mut()
            .find(|a| a.site.to_lowercase() == lower)
        {
            existing.username = username.to_string();
            existing.email = email.to_string();
        } else {
            self.profile.accounts.push(AccountEntry {
                site: site.to_string(),
                username: username.to_string(),
                email: email.to_string(),
            });
        }
    }

    /// Return a safe context string suitable for inclusion in LLM prompts.
    ///
    /// Excludes sensitive data: system_pin, full account credentials, and
    /// date of birth.
    pub fn get_safe_context(&self) -> String {
        let mut parts = Vec::new();
        if !self.profile.name.is_empty() {
            parts.push(format!("Name: {}", self.profile.name));
        }
        if !self.profile.email.is_empty() {
            parts.push(format!("Email: {}", self.profile.email));
        }
        if !self.profile.address.city.is_empty() {
            parts.push(format!(
                "Location: {}, {}",
                self.profile.address.city, self.profile.address.state
            ));
        }
        if !self.profile.accounts.is_empty() {
            let sites: Vec<&str> = self.profile.accounts.iter().map(|a| a.site.as_str()).collect();
            parts.push(format!("Accounts: {}", sites.join(", ")));
        }
        if parts.is_empty() {
            return "No user profile configured.".to_string();
        }
        parts.join("\n")
    }

    /// Persist the profile to disk as a JSON envelope.
    pub fn save(&self) -> Result<(), String> {
        let data = serde_json::to_string_pretty(&self.profile)
            .map_err(|e| format!("Failed to serialize profile: {e}"))?;

        let envelope = ProfileEnvelope {
            version: 1,
            encrypted: false,
            data,
        };

        let json = serde_json::to_string_pretty(&envelope)
            .map_err(|e| format!("Failed to serialize envelope: {e}"))?;

        // Ensure parent directory exists.
        if let Some(parent) = Path::new(&self.path).parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {e}"))?;
        }

        fs::write(&self.path, json).map_err(|e| format!("Failed to write profile: {e}"))?;
        Ok(())
    }

    /// Load the profile from disk.
    pub fn load(&mut self) -> Result<(), String> {
        let json =
            fs::read_to_string(&self.path).map_err(|e| format!("Failed to read profile: {e}"))?;

        let envelope: ProfileEnvelope = serde_json::from_str(&json)
            .map_err(|e| format!("Failed to parse envelope: {e}"))?;

        if envelope.encrypted {
            return Err("Encrypted profiles not yet supported in Rust port".to_string());
        }

        self.profile = serde_json::from_str(&envelope.data)
            .map_err(|e| format!("Failed to parse profile data: {e}"))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_path(name: &str) -> String {
        let mut path = env::temp_dir();
        path.push(format!("titan_profile_test_{}", name));
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn test_default_profile() {
        let mgr = UserProfileManager::new(&temp_path("default_does_not_exist.json"));
        let profile = mgr.get_profile();
        assert!(profile.name.is_empty());
        assert!(profile.accounts.is_empty());
    }

    #[test]
    fn test_update_profile_name() {
        let mut mgr = UserProfileManager::new(&temp_path("update_name.json"));
        let updated = mgr.update_profile(serde_json::json!({
            "name": "Alice",
            "email": "alice@example.com"
        }));
        assert_eq!(updated.name, "Alice");
        assert_eq!(updated.email, "alice@example.com");
    }

    #[test]
    fn test_update_profile_address() {
        let mut mgr = UserProfileManager::new(&temp_path("update_addr.json"));
        mgr.update_profile(serde_json::json!({
            "address": {
                "city": "Austin",
                "state": "TX"
            }
        }));
        assert_eq!(mgr.get_profile().address.city, "Austin");
        assert_eq!(mgr.get_profile().address.state, "TX");
    }

    #[test]
    fn test_add_account() {
        let mut mgr = UserProfileManager::new(&temp_path("add_account.json"));
        mgr.add_or_update_account("github.com", "alice", "alice@gh.com");
        let cred = mgr.get_credentials_for_site("github.com");
        assert!(cred.is_some());
        assert_eq!(cred.unwrap().username, "alice");
    }

    #[test]
    fn test_update_existing_account() {
        let mut mgr = UserProfileManager::new(&temp_path("update_account.json"));
        mgr.add_or_update_account("github.com", "alice", "alice@gh.com");
        mgr.add_or_update_account("github.com", "alice2", "alice2@gh.com");
        let cred = mgr.get_credentials_for_site("github.com").unwrap();
        assert_eq!(cred.username, "alice2");
        assert_eq!(mgr.get_profile().accounts.len(), 1);
    }

    #[test]
    fn test_credentials_case_insensitive() {
        let mut mgr = UserProfileManager::new(&temp_path("case_creds.json"));
        mgr.add_or_update_account("GitHub.com", "alice", "alice@gh.com");
        assert!(mgr.get_credentials_for_site("github.com").is_some());
        assert!(mgr.get_credentials_for_site("GITHUB.COM").is_some());
    }

    #[test]
    fn test_credentials_not_found() {
        let mgr = UserProfileManager::new(&temp_path("no_creds.json"));
        assert!(mgr.get_credentials_for_site("unknown.com").is_none());
    }

    #[test]
    fn test_safe_context_excludes_sensitive() {
        let mut mgr = UserProfileManager::new(&temp_path("safe_ctx.json"));
        mgr.update_profile(serde_json::json!({
            "name": "Bob",
            "system_pin": "1234",
            "date_of_birth": "1990-01-01"
        }));
        let ctx = mgr.get_safe_context();
        assert!(ctx.contains("Bob"));
        assert!(!ctx.contains("1234"));
        assert!(!ctx.contains("1990"));
    }

    #[test]
    fn test_safe_context_empty_profile() {
        let mgr = UserProfileManager::new(&temp_path("empty_ctx.json"));
        assert_eq!(mgr.get_safe_context(), "No user profile configured.");
    }

    #[test]
    fn test_save_and_load() {
        let path = temp_path("save_load_profile.json");

        {
            let mut mgr = UserProfileManager::new(&path);
            mgr.update_profile(serde_json::json!({
                "name": "Charlie",
                "email": "charlie@example.com"
            }));
            mgr.add_or_update_account("gitlab.com", "charlie", "c@gl.com");
            mgr.save().expect("save should succeed");
        }

        {
            let mgr = UserProfileManager::new(&path);
            let profile = mgr.get_profile();
            assert_eq!(profile.name, "Charlie");
            assert_eq!(profile.email, "charlie@example.com");
            assert_eq!(profile.accounts.len(), 1);
            assert_eq!(profile.accounts[0].site, "gitlab.com");
        }

        // Cleanup
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_profile_serialization() {
        let profile = UserProfile {
            name: "Test".to_string(),
            email: "test@test.com".to_string(),
            phone: "555-0100".to_string(),
            date_of_birth: "2000-01-01".to_string(),
            address: Address {
                street: "123 Main".to_string(),
                city: "Springfield".to_string(),
                state: "IL".to_string(),
                zip_code: "62701".to_string(),
            },
            accounts: vec![AccountEntry {
                site: "github.com".to_string(),
                username: "tester".to_string(),
                email: "tester@gh.com".to_string(),
            }],
            google_account: "test@gmail.com".to_string(),
            system_pin: "0000".to_string(),
        };

        let json = serde_json::to_string(&profile).unwrap();
        let restored: UserProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, "Test");
        assert_eq!(restored.address.city, "Springfield");
        assert_eq!(restored.accounts.len(), 1);
    }
}
