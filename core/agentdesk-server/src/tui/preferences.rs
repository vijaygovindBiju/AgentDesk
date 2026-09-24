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

        let prefs = WorkspacePreferences {
            last_agent_id: Some("antigravity".into()),
            last_workspace: Some(PathBuf::from("/tmp/test-project")),
        };

        save_preferences(config_dir, &prefs).unwrap();

        let loaded = load_preferences(config_dir);
        assert_eq!(loaded, prefs);
    }
}
