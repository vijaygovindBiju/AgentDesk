//! Automatic coding-agent discovery architecture.
//! Discovers installed coding agents on the user's system by inspecting PATH
//! and returns normalized agent definitions.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use serde::{Deserialize, Serialize};

/// Operational availability of an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentAvailability {
    /// Agent is installed and AgentDesk has a native adapter for it.
    Ready,
    /// Agent is installed, but AgentDesk does not yet have an adapter for it.
    Unsupported { reason: String },
}

/// The adapter technology used to supervise the coding agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdapterType {
    /// Real POSIX PTY with VT100 screen parsing and semantic attention detection.
    AntigravityPty,
    /// Agent Client Protocol (ACP) over stdio JSON-RPC.
    Acp,
    /// No adapter available.
    None,
}

/// Normalized description of a discovered coding agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Unique identifier for the agent (e.g. "antigravity", "claude", "codex", "gemini").
    pub id: String,
    /// Human-readable display name (e.g. "Antigravity", "Claude Code", "Codex", "Gemini CLI").
    pub display_name: String,
    /// Name of the primary executable binary (e.g. "agy", "claude", "codex", "gemini").
    pub executable: String,
    /// Absolute filesystem path to the discovered executable.
    pub executable_path: PathBuf,
    /// Resolved version string if available (e.g. "1.2.10").
    pub version: Option<String>,
    /// Availability status for supervision.
    pub availability: AgentAvailability,
    /// Human-readable capability descriptors.
    pub capabilities: Vec<String>,
    /// Adapter technology type.
    pub adapter_type: AdapterType,
}

impl AgentDefinition {
    pub fn is_ready(&self) -> bool {
        matches!(self.availability, AgentAvailability::Ready)
    }

    pub fn status_text(&self) -> &str {
        match &self.availability {
            AgentAvailability::Ready => "Ready",
            AgentAvailability::Unsupported { .. } => "Unsupported",
        }
    }
}

/// Specification template for known coding agents.
#[derive(Debug, Clone)]
struct AgentTemplate {
    id: &'static str,
    display_name: &'static str,
    executable: &'static str,
    adapter_type: AdapterType,
    unsupported_reason: Option<&'static str>,
    capabilities: &'static [&'static str],
}

const KNOWN_AGENTS: &[AgentTemplate] = &[
    AgentTemplate {
        id: "antigravity",
        display_name: "Antigravity",
        executable: "agy",
        adapter_type: AdapterType::AntigravityPty,
        unsupported_reason: None,
        capabilities: &[
            "Interactive PTY",
            "Command Approvals",
            "Structured Questions",
            "Workspace Trust",
        ],
    },
    AgentTemplate {
        id: "gemini",
        display_name: "Gemini CLI",
        executable: "gemini",
        adapter_type: AdapterType::Acp,
        unsupported_reason: None,
        capabilities: &["ACP Protocol", "Permission Prompts", "Turn Management"],
    },
    AgentTemplate {
        id: "claude",
        display_name: "Claude Code",
        executable: "claude",
        adapter_type: AdapterType::None,
        unsupported_reason: Some("No adapter available yet"),
        capabilities: &["CLI Tool"],
    },
    AgentTemplate {
        id: "codex",
        display_name: "Codex",
        executable: "codex",
        adapter_type: AdapterType::None,
        unsupported_reason: Some("No adapter available yet"),
        capabilities: &["CLI Tool"],
    },
];

/// Discovery engine for coding agents.
pub struct AgentDiscovery;

impl AgentDiscovery {
    /// Discover all installed coding agents by searching the system PATH.
    pub fn discover() -> Vec<AgentDefinition> {
        let path_var = std::env::var_os("PATH").unwrap_or_default();
        let paths: Vec<PathBuf> = std::env::split_paths(&path_var).collect();
        Self::discover_in_paths(&paths)
    }

    /// Discover agents in a specific list of directories.
    pub fn discover_in_paths(paths: &[PathBuf]) -> Vec<AgentDefinition> {
        let mut discovered = Vec::new();
        let mut seen_ids = HashSet::new();

        // Deduplicate and filter valid search directories
        let mut unique_paths = Vec::new();
        let mut seen_dirs = HashSet::new();
        for p in paths {
            if let Ok(canonical) = p.canonicalize() {
                if seen_dirs.insert(canonical.clone()) {
                    unique_paths.push(canonical);
                }
            } else if seen_dirs.insert(p.clone()) {
                unique_paths.push(p.clone());
            }
        }

        for template in KNOWN_AGENTS {
            if seen_ids.contains(template.id) {
                continue;
            }

            for dir in &unique_paths {
                let candidate = dir.join(template.executable);
                if is_executable(&candidate) {
                    let version = query_version(&candidate);
                    let availability = match template.unsupported_reason {
                        Some(reason) => AgentAvailability::Unsupported {
                            reason: reason.to_string(),
                        },
                        None => AgentAvailability::Ready,
                    };

                    let def = AgentDefinition {
                        id: template.id.to_string(),
                        display_name: template.display_name.to_string(),
                        executable: template.executable.to_string(),
                        executable_path: candidate.canonicalize().unwrap_or(candidate),
                        version,
                        availability,
                        capabilities: template.capabilities.iter().map(|s| s.to_string()).collect(),
                        adapter_type: template.adapter_type,
                    };

                    seen_ids.insert(template.id);
                    discovered.push(def);
                    break;
                }
            }
        }

        discovered
    }
}

/// Check if a path points to an executable regular file.
fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = path.metadata() {
            return metadata.permissions().mode() & 0o111 != 0;
        }
    }

    #[cfg(not(unix))]
    {
        if let Ok(metadata) = path.metadata() {
            return !metadata.permissions().readonly();
        }
    }

    false
}

/// Run executable with `--version` and parse the output.
fn query_version(path: &Path) -> Option<String> {
    let output = Command::new(path).arg("--version").output().ok()?;

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout);
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            // Take the first line if multi-line
            let first_line = trimmed.lines().next().unwrap_or(trimmed).trim();
            return Some(first_line.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn create_dummy_executable(dir: &Path, name: &str) -> PathBuf {
        let file_path = dir.join(name);
        File::create(&file_path).unwrap();
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&file_path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&file_path, perms).unwrap();
        }
        file_path
    }

    #[test]
    fn test_discover_in_empty_paths() {
        let agents = AgentDiscovery::discover_in_paths(&[]);
        assert!(agents.is_empty());
    }

    #[test]
    fn test_discover_finds_supported_and_unsupported_agents() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path();

        create_dummy_executable(path, "agy");
        create_dummy_executable(path, "claude");
        create_dummy_executable(path, "codex");

        let agents = AgentDiscovery::discover_in_paths(&[path.to_path_buf()]);

        assert_eq!(agents.len(), 3);

        let agy = agents.iter().find(|a| a.id == "antigravity").unwrap();
        assert_eq!(agy.display_name, "Antigravity");
        assert_eq!(agy.executable, "agy");
        assert!(agy.is_ready());
        assert_eq!(agy.adapter_type, AdapterType::AntigravityPty);

        let claude = agents.iter().find(|a| a.id == "claude").unwrap();
        assert_eq!(claude.display_name, "Claude Code");
        assert!(!claude.is_ready());
        assert_eq!(claude.adapter_type, AdapterType::None);

        let codex = agents.iter().find(|a| a.id == "codex").unwrap();
        assert_eq!(codex.display_name, "Codex");
        assert!(!codex.is_ready());
        assert_eq!(codex.adapter_type, AdapterType::None);
    }

    #[test]
    fn test_discover_ignores_non_executable_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path();

        let agy_path = path.join("agy");
        File::create(&agy_path).unwrap();
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&agy_path).unwrap().permissions();
            perms.set_mode(0o644); // Not executable
            fs::set_permissions(&agy_path, perms).unwrap();
        }

        let agents = AgentDiscovery::discover_in_paths(&[path.to_path_buf()]);
        assert!(agents.is_empty());
    }

    #[test]
    fn test_discover_handles_duplicate_path_entries() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path();

        create_dummy_executable(path, "agy");

        let duplicate_paths = vec![path.to_path_buf(), path.to_path_buf(), path.to_path_buf()];
        let agents = AgentDiscovery::discover_in_paths(&duplicate_paths);

        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id, "antigravity");
    }
}
