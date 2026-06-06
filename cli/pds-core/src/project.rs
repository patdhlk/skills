use std::path::{Path, PathBuf};

use crate::error::Error;

const CONFIG_FILE: &str = "ubproject.toml";

/// A resolved sphinx-needs project: the directory holding `ubproject.toml`.
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub config_path: PathBuf,
}

impl Project {
    /// Walk up from `start` until an `ubproject.toml` is found, git-style.
    pub fn discover(start: &Path) -> Result<Project, Error> {
        for dir in start.ancestors() {
            let candidate = dir.join(CONFIG_FILE);
            if candidate.is_file() {
                return Ok(Project {
                    root: dir.to_path_buf(),
                    config_path: candidate,
                });
            }
        }
        Err(Error::Config {
            message: format!(
                "no {CONFIG_FILE} found in {} or any parent directory",
                start.display()
            ),
        })
    }

    /// Resolve a project from an explicit config path (the `--config` override).
    /// The file's parent directory becomes the project root.
    pub fn from_config_path(path: &Path) -> Result<Project, Error> {
        if !path.is_file() {
            return Err(Error::Config {
                message: format!("config file not found: {}", path.display()),
            });
        }
        let root = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Project {
            root,
            config_path: path.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_finds_config_in_ancestor() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::fs::write(root.join(CONFIG_FILE), "").unwrap();
        let nested = root.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();

        let project = Project::discover(&nested).unwrap();

        assert_eq!(project.root, root);
        assert_eq!(project.config_path, root.join(CONFIG_FILE));
    }

    #[test]
    fn discover_without_config_is_config_error() {
        let tmp = tempfile::tempdir().unwrap();
        let err = Project::discover(tmp.path()).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
    }

    #[test]
    fn from_config_path_uses_parent_as_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let config = root.join(CONFIG_FILE);
        std::fs::write(&config, "").unwrap();

        let project = Project::from_config_path(&config).unwrap();

        assert_eq!(project.root, root);
        assert_eq!(project.config_path, config);
    }

    #[test]
    fn from_config_path_missing_is_config_error() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope.toml");
        let err = Project::from_config_path(&missing).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
    }
}
