//! User preferences persistence (last used agent, last used workspace/project).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WorkspacePreferences {
    /// ID of the last selected coding agent (e.g. "antigravity").
    pub last_agent_id: Option<String>,
    /// Last used workspace / project directory.
    pub last_workspace: Option<PathBuf>,
    /// Whether --dangerously-skip-permissions is enabled.
    #[serde(default)]
    pub dangerously_skip_permissions: bool,
    /// Recent workspace directories.
    #[serde(default)]
    pub recent_workspaces: Vec<PathBuf>,
}

impl WorkspacePreferences {
    /// Add or update a workspace path at the front of recent list (max 10 entries).
    pub fn add_recent_workspace(&mut self, path: PathBuf) {
        self.recent_workspaces.retain(|p| p != &path);
        self.recent_workspaces.insert(0, path);
        if self.recent_workspaces.len() > 10 {
            self.recent_workspaces.truncate(10);
        }
    }
}

pub fn preferences_path(config_dir: &Path) -> PathBuf {
    config_dir.join("preferences.json")
}

pub fn load_preferences(config_dir: &Path) -> WorkspacePreferences {
    let path = preferences_path(config_dir);
    if !path.exists() {
        return WorkspacePreferences::default();
    }
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => WorkspacePreferences::default(),
    }
}

pub fn save_preferences(config_dir: &Path, prefs: &WorkspacePreferences) -> io::Result<()> {
    fs::create_dir_all(config_dir)?;
    let path = preferences_path(config_dir);
    let json = serde_json::to_string_pretty(prefs)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preferences_save_and_load() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path();

        let initial = load_preferences(config_dir);
        assert_eq!(initial, WorkspacePreferences::default());

        let mut prefs = WorkspacePreferences {
            last_agent_id: Some("antigravity".into()),
            last_workspace: Some(PathBuf::from("/tmp/test-project")),
            dangerously_skip_permissions: true,
            recent_workspaces: vec![PathBuf::from("/tmp/test-project")],
        };
        prefs.add_recent_workspace(PathBuf::from("/tmp/another-project"));

        save_preferences(config_dir, &prefs).unwrap();

        let loaded = load_preferences(config_dir);
        assert_eq!(loaded, prefs);
        assert!(loaded.dangerously_skip_permissions);
        assert_eq!(loaded.recent_workspaces.len(), 2);
        assert_eq!(loaded.recent_workspaces[0], PathBuf::from("/tmp/another-project"));
    }
}
