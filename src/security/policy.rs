//! Attribute-Based Access Control (ABAC) Policy Manager.
//!
//! Ported from `sovereign_titan/security/policy.py`.
//! Every tool invocation in the ReAct loop must pass through
//! `PolicyManager::check_permission()` before execution.
//! Blocks destructive operations unless the user profile grants
//! the required clearance level.

use std::collections::HashSet;

use tracing::{info, warn};

/// Clearance levels (ascending privilege).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Clearance {
    /// Basic user — can read files, search, launch apps.
    User,
    /// Power user — can write files, run safe shell commands.
    PowerUser,
    /// Administrator — can modify system settings, kill processes.
    Admin,
    /// System administrator — unrestricted, including destructive ops.
    SysAdmin,
}

impl Clearance {
    /// Parse a clearance level from a string.
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "sysadmin" | "sys_admin" | "root" => Clearance::SysAdmin,
            "admin" | "administrator" => Clearance::Admin,
            "poweruser" | "power_user" | "power" => Clearance::PowerUser,
            _ => Clearance::User,
        }
    }
}

/// The result of a permission check.
#[derive(Debug, Clone, PartialEq)]
pub enum PolicyDecision {
    /// Action is allowed.
    Allow,
    /// Action is denied with a reason.
    Deny { reason: String },
}

/// A policy rule that matches tool+action combinations.
#[derive(Debug, Clone)]
struct PolicyRule {
    /// Tool name pattern (exact match or "*" for any).
    tool: String,
    /// Blocked input patterns (substrings that trigger denial).
    blocked_patterns: Vec<String>,
    /// Minimum clearance required for this tool.
    min_clearance: Clearance,
}

/// ABAC Policy Manager — gates tool execution in the ReAct loop.
///
/// Maintains a set of rules that map tool names to minimum clearance
/// levels and blocked input patterns. Each tool invocation is checked
/// against these rules before execution.
pub struct PolicyManager {
    /// The current user's clearance level.
    clearance: Clearance,
    /// Policy rules indexed by tool name.
    rules: Vec<PolicyRule>,
    /// Globally blocked command patterns (applied to all tools).
    /// Uses `HashSet` for O(1) membership checks.
    global_blocks: HashSet<String>,
    /// Audit log of recent decisions (tool, action, decision).
    audit_log: Vec<(String, String, String)>,
    /// Maximum audit log entries.
    max_audit: usize,
}

impl PolicyManager {
    /// Create a new policy manager with the given clearance and default rules.
    pub fn new(clearance: Clearance) -> Self {
        let mut pm = Self {
            clearance,
            rules: Vec::new(),
            global_blocks: HashSet::new(),
            audit_log: Vec::new(),
            max_audit: 1000,
        };
        pm.load_default_rules();
        pm
    }

    /// Load the default security rules.
    fn load_default_rules(&mut self) {
        // ── Global blocks (any tool) ────────────────────────────────────
        self.global_blocks = HashSet::from([
            // Catastrophic file system ops
            "rm -rf /".to_string(),
            "rm -rf /*".to_string(),
            "rmdir /s /q c:\\".to_string(),
            "format c:".to_string(),
            "del /f /s /q c:\\".to_string(),
            // Registry destruction
            "reg delete hklm".to_string(),
            "reg delete hkcu".to_string(),
            "reg delete hkcr".to_string(),
            // Credential theft
            "mimikatz".to_string(),
            "sekurlsa".to_string(),
            "lsadump".to_string(),
            // Network pivoting
            "nc -e".to_string(),
            "ncat -e".to_string(),
            // Disabling security
            "set-mppreference -disablerealtimemonitoring".to_string(),
            "netsh advfirewall set allprofiles state off".to_string(),
            // Ransomware-like patterns
            "cipher /w".to_string(),
            "vssadmin delete shadows".to_string(),
            "wmic shadowcopy delete".to_string(),
            "bcdedit /set.*recoveryenabled no".to_string(),
        ]);

        // ── Shell tool — most dangerous, requires PowerUser minimum ─────
        self.rules.push(PolicyRule {
            tool: "shell".to_string(),
            blocked_patterns: vec![
                // Additional shell-specific blocks
                "powershell -enc".to_string(),      // Encoded commands (obfuscation)
                "invoke-expression".to_string(),     // IEX (code injection)
                "downloadstring".to_string(),        // Remote code download
                "invoke-webrequest".to_string(),     // Could be used for exfil
                "new-service".to_string(),           // Service persistence
                "schtasks /create".to_string(),      // Scheduled task persistence
                "reg add.*\\run".to_string(),        // Startup persistence
                "net user.*add".to_string(),         // Creating rogue users
                "net localgroup administrators".to_string(), // Privilege escalation
            ],
            min_clearance: Clearance::PowerUser,
        });

        // ── System control — Admin for kill/service ops ─────────────────
        self.rules.push(PolicyRule {
            tool: "system_control".to_string(),
            blocked_patterns: vec![
                // Critical system processes — never allow killing these.
                "lsass".to_string(),
                "csrss".to_string(),
                "winlogon".to_string(),
                "services.exe".to_string(),
                "smss".to_string(),
                "wininit".to_string(),
            ],
            min_clearance: Clearance::Admin,
        });

        // ── Computer control — User level (UI automation) ───────────────
        self.rules.push(PolicyRule {
            tool: "computer_control".to_string(),
            blocked_patterns: vec![],
            min_clearance: Clearance::User,
        });

        // ── Code ops — PowerUser for write, User for read ───────────────
        self.rules.push(PolicyRule {
            tool: "code_ops".to_string(),
            blocked_patterns: vec![
                // Block writing to system directories (also enforced in code_ops itself)
                "c:\\\\windows".to_string(),
                "c:\\\\program files".to_string(),
                "system32".to_string(),
            ],
            min_clearance: Clearance::User,
        });

        // ── Web search — User level ────────────────────────────────────
        self.rules.push(PolicyRule {
            tool: "web_search".to_string(),
            blocked_patterns: vec![],
            min_clearance: Clearance::User,
        });

        // ── File search — User level ───────────────────────────────────
        self.rules.push(PolicyRule {
            tool: "file_search".to_string(),
            blocked_patterns: vec![],
            min_clearance: Clearance::User,
        });
    }

    /// Check if a tool invocation is permitted.
    ///
    /// # Arguments
    /// * `tool_name` — The name of the tool being invoked.
    /// * `action_input` — The raw input string for the tool.
    ///
    /// # Returns
    /// `PolicyDecision::Allow` if permitted, `PolicyDecision::Deny` with reason if blocked.
    pub fn check_permission(&mut self, tool_name: &str, action_input: &str) -> PolicyDecision {
        let input_lower = action_input.to_lowercase();
        let tool_lower = tool_name.to_lowercase();

        // ── Check global blocks first ───────────────────────────────────
        for pattern in &self.global_blocks {
            if input_lower.contains(&pattern.to_lowercase()) {
                let reason = format!(
                    "BLOCKED: globally prohibited pattern '{}' in tool '{}'",
                    pattern, tool_name
                );
                warn!("Policy: {reason}");
                self.log_decision(tool_name, action_input, "DENY(global)");
                return PolicyDecision::Deny { reason };
            }
        }

        // ── Find matching tool rule ─────────────────────────────────────
        if let Some(rule) = self.rules.iter().find(|r| r.tool == tool_lower) {
            // Check clearance level.
            if self.clearance < rule.min_clearance {
                let reason = format!(
                    "BLOCKED: tool '{}' requires {:?} clearance, you have {:?}",
                    tool_name, rule.min_clearance, self.clearance
                );
                warn!("Policy: {reason}");
                self.log_decision(tool_name, action_input, "DENY(clearance)");
                return PolicyDecision::Deny { reason };
            }

            // Check tool-specific blocked patterns.
            for pattern in &rule.blocked_patterns {
                if input_lower.contains(&pattern.to_lowercase()) {
                    // SYS_ADMIN can override tool-specific blocks.
                    if self.clearance >= Clearance::SysAdmin {
                        info!(
                            "Policy: SYS_ADMIN override for pattern '{}' on tool '{}'",
                            pattern, tool_name
                        );
                        continue;
                    }

                    let reason = format!(
                        "BLOCKED: prohibited pattern '{}' for tool '{}'",
                        pattern, tool_name
                    );
                    warn!("Policy: {reason}");
                    self.log_decision(tool_name, action_input, "DENY(pattern)");
                    return PolicyDecision::Deny { reason };
                }
            }
        }
        // If no rule matches, default to requiring Admin clearance for unknown tools.
        else if self.clearance < Clearance::Admin {
            let reason = format!(
                "BLOCKED: unknown tool '{}' requires Admin clearance (got {:?})",
                tool_name, self.clearance
            );
            warn!("Policy: {reason}");
            self.log_decision(tool_name, action_input, "DENY(unknown_tool)");
            return PolicyDecision::Deny { reason };
        }

        self.log_decision(tool_name, action_input, "ALLOW");
        PolicyDecision::Allow
    }

    /// Update the user's clearance level.
    pub fn set_clearance(&mut self, clearance: Clearance) {
        info!("Policy: clearance changed from {:?} to {:?}", self.clearance, clearance);
        self.clearance = clearance;
    }

    /// Get the current clearance level.
    pub fn clearance(&self) -> Clearance {
        self.clearance
    }

    /// Get recent audit log entries.
    pub fn audit_log(&self) -> &[(String, String, String)] {
        &self.audit_log
    }

    /// Record a decision in the audit log.
    fn log_decision(&mut self, tool: &str, input: &str, decision: &str) {
        // Truncate input for logging.
        let short_input = if input.len() > 100 {
            format!("{}...", &input[..100])
        } else {
            input.to_string()
        };

        self.audit_log.push((
            tool.to_string(),
            short_input,
            decision.to_string(),
        ));

        if self.audit_log.len() > self.max_audit {
            self.audit_log.remove(0);
        }
    }
}

impl Default for PolicyManager {
    fn default() -> Self {
        Self::new(Clearance::User)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_can_search() {
        let mut pm = PolicyManager::new(Clearance::User);
        assert_eq!(
            pm.check_permission("file_search", "{\"query\": \"readme.md\"}"),
            PolicyDecision::Allow
        );
    }

    #[test]
    fn test_user_blocked_from_shell() {
        let mut pm = PolicyManager::new(Clearance::User);
        match pm.check_permission("shell", "{\"command\": \"dir\"}") {
            PolicyDecision::Deny { reason } => {
                assert!(reason.contains("clearance"));
            }
            PolicyDecision::Allow => panic!("User should not have shell access"),
        }
    }

    #[test]
    fn test_poweruser_can_shell() {
        let mut pm = PolicyManager::new(Clearance::PowerUser);
        assert_eq!(
            pm.check_permission("shell", "{\"command\": \"dir\"}"),
            PolicyDecision::Allow
        );
    }

    #[test]
    fn test_global_block_rm_rf() {
        let mut pm = PolicyManager::new(Clearance::SysAdmin);
        match pm.check_permission("shell", "{\"command\": \"rm -rf /\"}") {
            PolicyDecision::Deny { reason } => {
                assert!(reason.contains("globally prohibited"));
            }
            PolicyDecision::Allow => panic!("rm -rf / should always be blocked"),
        }
    }

    #[test]
    fn test_global_block_registry_delete() {
        let mut pm = PolicyManager::new(Clearance::SysAdmin);
        match pm.check_permission("shell", "reg delete HKLM\\Software") {
            PolicyDecision::Deny { reason } => {
                assert!(reason.contains("globally prohibited"));
            }
            PolicyDecision::Allow => panic!("registry deletion should be blocked"),
        }
    }

    #[test]
    fn test_sysadmin_overrides_tool_patterns() {
        let mut pm = PolicyManager::new(Clearance::SysAdmin);
        // SysAdmin can override tool-specific blocks (but NOT global blocks).
        assert_eq!(
            pm.check_permission("shell", "powershell -enc base64string"),
            PolicyDecision::Allow
        );
    }

    #[test]
    fn test_admin_blocked_encoded_powershell() {
        let mut pm = PolicyManager::new(Clearance::Admin);
        match pm.check_permission("shell", "powershell -enc base64string") {
            PolicyDecision::Deny { reason } => {
                assert!(reason.contains("prohibited pattern"));
            }
            PolicyDecision::Allow => panic!("encoded powershell should be blocked for Admin"),
        }
    }

    #[test]
    fn test_kill_critical_process_blocked() {
        let mut pm = PolicyManager::new(Clearance::Admin);
        match pm.check_permission("system_control", "{\"action\": \"kill_process\", \"name\": \"lsass\"}") {
            PolicyDecision::Deny { reason } => {
                assert!(reason.contains("prohibited pattern"));
            }
            PolicyDecision::Allow => panic!("killing lsass should be blocked"),
        }
    }

    #[test]
    fn test_unknown_tool_requires_admin() {
        let mut pm = PolicyManager::new(Clearance::User);
        match pm.check_permission("unknown_tool", "{}") {
            PolicyDecision::Deny { reason } => {
                assert!(reason.contains("unknown tool"));
            }
            PolicyDecision::Allow => panic!("unknown tools should require Admin"),
        }
    }

    #[test]
    fn test_clearance_ordering() {
        assert!(Clearance::User < Clearance::PowerUser);
        assert!(Clearance::PowerUser < Clearance::Admin);
        assert!(Clearance::Admin < Clearance::SysAdmin);
    }

    #[test]
    fn test_clearance_from_str() {
        assert_eq!(Clearance::from_str_loose("sysadmin"), Clearance::SysAdmin);
        assert_eq!(Clearance::from_str_loose("admin"), Clearance::Admin);
        assert_eq!(Clearance::from_str_loose("poweruser"), Clearance::PowerUser);
        assert_eq!(Clearance::from_str_loose("user"), Clearance::User);
        assert_eq!(Clearance::from_str_loose("root"), Clearance::SysAdmin);
        assert_eq!(Clearance::from_str_loose("random"), Clearance::User);
    }

    #[test]
    fn test_audit_log() {
        let mut pm = PolicyManager::new(Clearance::User);
        pm.check_permission("file_search", "test");
        assert_eq!(pm.audit_log().len(), 1);
        assert_eq!(pm.audit_log()[0].2, "ALLOW");
    }

    #[test]
    fn test_set_clearance() {
        let mut pm = PolicyManager::new(Clearance::User);
        match pm.check_permission("shell", "dir") {
            PolicyDecision::Deny { .. } => {}
            _ => panic!("should be denied"),
        }
        pm.set_clearance(Clearance::PowerUser);
        assert_eq!(
            pm.check_permission("shell", "dir"),
            PolicyDecision::Allow
        );
    }

    #[test]
    fn test_vssadmin_blocked() {
        let mut pm = PolicyManager::new(Clearance::SysAdmin);
        match pm.check_permission("shell", "vssadmin delete shadows /all") {
            PolicyDecision::Deny { reason } => {
                assert!(reason.contains("globally prohibited"));
            }
            PolicyDecision::Allow => panic!("shadow deletion should be globally blocked"),
        }
    }
}
