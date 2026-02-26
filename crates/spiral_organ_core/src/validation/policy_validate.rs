use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;

const POLICY_API_VERSION: &str = "spiralorgan.policy/v0.1";

const READ_ONLY_COMMANDS: &[&str] = &[
    "cat", "find", "head", "ls", "pwd", "rg", "sed", "tail", "wc",
];
const NO_SANDBOX_STRICT_COMMANDS: &[&str] = &["cargo", "cat", "git", "ls", "rg", "sed"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicySeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyIssue {
    pub code: &'static str,
    pub severity: PolicySeverity,
    pub message: String,
}

impl PolicyIssue {
    fn err(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: PolicySeverity::Error,
            message: message.into(),
        }
    }

    fn warn(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: PolicySeverity::Warning,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PolicySpec {
    pub api_version: String,
    pub mode: PolicyMode,
    pub features: FeatureFlags,
    pub policy: PolicyRules,
    #[serde(default)]
    pub audit: AuditConfig,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PolicyMode {
    Readonly,
    Supervised,
    Full,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FeatureFlags {
    pub channels: EnabledFlag,
    pub plugins: EnabledFlag,
    pub sandbox: SandboxFlag,
    pub pipeline_edit: EnabledFlag,
    pub token_distill: EnabledFlag,
    pub vision_snapshot: EnabledFlag,
    pub worlddefine_bridge: EnabledFlag,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EnabledFlag {
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SandboxFlag {
    pub enabled: bool,
    pub backend: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PolicyRules {
    pub workspace_only: bool,
    pub allowed_commands: Vec<String>,
    pub forbidden_paths: Vec<String>,
    pub risk_matrix: RiskMatrix,
    pub enforcement: EnforcementRules,
    pub approval: ApprovalRules,
    pub rate_limits: RateLimits,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RiskMatrix {
    #[serde(default)]
    pub shell_exec: String,
    #[serde(default)]
    pub file_write_outside_workspace: String,
    #[serde(default)]
    pub browser_remote_control: String,
    #[serde(default)]
    pub channel_bind: String,
    #[serde(default)]
    pub plugin_install: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct EnforcementRules {
    #[serde(default)]
    pub low: String,
    #[serde(default)]
    pub medium: String,
    #[serde(default)]
    pub high: String,
    #[serde(default)]
    pub critical: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRules {
    #[serde(default)]
    pub required_for: Vec<String>,
    #[serde(default)]
    pub channel_mode: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RateLimits {
    pub per_user_per_minute: u32,
    pub per_channel_per_minute: u32,
    pub per_session_parallel_actions: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AuditConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub redact_secrets: bool,
    #[serde(default = "default_export_format")]
    pub export_format: String,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            redact_secrets: true,
            export_format: default_export_format(),
        }
    }
}

pub fn parse_policy_toml(input: &str) -> Result<PolicySpec, String> {
    toml::from_str(input).map_err(|e| format!("invalid policy toml: {e}"))
}

pub fn load_and_validate_policy(path: impl AsRef<Path>) -> Result<Vec<PolicyIssue>, String> {
    let path_ref = path.as_ref();
    let content = std::fs::read_to_string(path_ref)
        .map_err(|e| format!("failed to read {}: {e}", path_ref.display()))?;
    let spec = parse_policy_toml(&content)?;
    Ok(validate_policy(&spec))
}

pub fn validate_policy(spec: &PolicySpec) -> Vec<PolicyIssue> {
    let mut issues = Vec::new();

    if spec.api_version != POLICY_API_VERSION {
        issues.push(PolicyIssue::err(
            "P100",
            format!(
                "unsupported api_version: {} (expected {})",
                spec.api_version, POLICY_API_VERSION
            ),
        ));
    }

    if spec.features.token_distill.enabled && !spec.audit.redact_secrets {
        issues.push(PolicyIssue::err(
            "P102",
            "token_distill requires audit.redact_secrets=true",
        ));
    }

    if matches!(spec.mode, PolicyMode::Readonly) {
        let invalid = find_outside_allowlist(&spec.policy.allowed_commands, READ_ONLY_COMMANDS);
        if !invalid.is_empty() {
            issues.push(PolicyIssue::err(
                "P101",
                format!(
                    "readonly mode allows read-only commands only, found: {}",
                    invalid.join(", ")
                ),
            ));
        }

        if spec.features.pipeline_edit.enabled {
            issues.push(PolicyIssue::warn(
                "P201",
                "pipeline_edit enabled in readonly mode: runtime should force dry-run/no-write behavior",
            ));
        }
    }

    if !spec.features.sandbox.enabled {
        if !spec.policy.workspace_only {
            issues.push(PolicyIssue::err(
                "P103",
                "sandbox disabled requires policy.workspace_only=true",
            ));
        }

        let invalid =
            find_outside_allowlist(&spec.policy.allowed_commands, NO_SANDBOX_STRICT_COMMANDS);
        if !invalid.is_empty() {
            issues.push(PolicyIssue::err(
                "P104",
                format!(
                    "sandbox disabled requires strict command allowlist, found: {}",
                    invalid.join(", ")
                ),
            ));
        }
    }

    if spec.features.sandbox.enabled && spec.features.sandbox.backend == "none" {
        issues.push(PolicyIssue::warn(
            "P202",
            "sandbox.enabled=true but backend=none; consider using auto/docker/landlock/firejail/bubblewrap",
        ));
    }

    if !spec.features.sandbox.enabled && spec.features.sandbox.backend != "none" {
        issues.push(PolicyIssue::warn(
            "P203",
            format!(
                "sandbox backend '{}' is ignored while sandbox.enabled=false",
                spec.features.sandbox.backend
            ),
        ));
    }

    issues
}

fn find_outside_allowlist(commands: &[String], allowlist: &[&str]) -> Vec<String> {
    let allowed: HashSet<_> = allowlist.iter().copied().collect();
    commands
        .iter()
        .map(|c| normalize_command(c))
        .filter(|c| !allowed.contains(c.as_str()))
        .collect()
}

fn normalize_command(command: &str) -> String {
    command
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim()
        .to_lowercase()
}

fn default_true() -> bool {
    true
}

fn default_export_format() -> String {
    "jsonl".to_string()
}

#[cfg(test)]
mod tests {
    use super::{PolicyMode, PolicySeverity, parse_policy_toml, validate_policy};

    #[test]
    fn default_policy_template_is_valid() {
        let spec = parse_policy_toml(include_str!(
            "../../../../.design/specs/policy.default.toml"
        ))
        .expect("default policy should parse");
        let issues = validate_policy(&spec);
        let has_error = issues.iter().any(|i| i.severity == PolicySeverity::Error);
        assert!(!has_error, "default policy should have no errors");
    }

    #[test]
    fn readonly_mode_rejects_mutating_commands() {
        let mut spec = parse_policy_toml(include_str!(
            "../../../../.design/specs/policy.default.toml"
        ))
        .expect("policy parse should pass");
        spec.mode = PolicyMode::Readonly;
        spec.policy.allowed_commands = vec!["cat".to_string(), "cargo".to_string()];
        let issues = validate_policy(&spec);
        assert!(
            issues.iter().any(|i| i.code == "P101"),
            "readonly violation should be reported"
        );
    }

    #[test]
    fn token_distill_requires_secret_redaction() {
        let mut spec = parse_policy_toml(include_str!(
            "../../../../.design/specs/policy.default.toml"
        ))
        .expect("policy parse should pass");
        spec.features.token_distill.enabled = true;
        spec.audit.redact_secrets = false;
        let issues = validate_policy(&spec);
        assert!(
            issues.iter().any(|i| i.code == "P102"),
            "token distill secrecy violation should be reported"
        );
    }

    #[test]
    fn sandbox_disabled_requires_strict_commands() {
        let mut spec = parse_policy_toml(include_str!(
            "../../../../.design/specs/policy.default.toml"
        ))
        .expect("policy parse should pass");
        spec.features.sandbox.enabled = false;
        spec.features.sandbox.backend = "none".to_string();
        spec.policy.allowed_commands = vec!["ls".to_string(), "bash".to_string()];
        let issues = validate_policy(&spec);
        assert!(
            issues.iter().any(|i| i.code == "P104"),
            "sandbox-disabled strict-command violation should be reported"
        );
    }
}
