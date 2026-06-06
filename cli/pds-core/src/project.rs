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
                    root: absolutize(dir)?,
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
    /// The file's parent directory becomes the project root. The root is always
    /// absolute, even when `path` is a bare filename like `ubproject.toml`.
    pub fn from_config_path(path: &Path) -> Result<Project, Error> {
        if !path.is_file() {
            return Err(Error::Config {
                message: format!("config file not found: {}", path.display()),
            });
        }
        // A bare filename has an empty parent (`Some("")`); treat that as the cwd.
        let parent = match path.parent() {
            Some(p) if p.as_os_str().is_empty() => Path::new("."),
            Some(p) => p,
            None => Path::new("."),
        };
        Ok(Project {
            root: absolutize(parent)?,
            config_path: path.to_path_buf(),
        })
    }
}

/// Resolve a directory to an absolute, canonical path so callers see a stable root
/// regardless of whether they passed a relative path or a bare filename.
fn absolutize(dir: &Path) -> Result<PathBuf, Error> {
    dir.canonicalize().map_err(|e| Error::Config {
        message: format!("cannot resolve project root {}: {e}", dir.display()),
    })
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

    /// A bare `--config ubproject.toml` must yield an absolute root (the cwd),
    /// matching what `discover` returns — never a relative `"."`.
    #[test]
    fn from_config_path_bare_filename_yields_absolute_root() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        std::fs::write(dir.join(CONFIG_FILE), "").unwrap();

        // Run from inside the temp dir so the bare filename resolves there.
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let result = Project::from_config_path(Path::new(CONFIG_FILE));
        std::env::set_current_dir(&original).unwrap();

        let project = result.unwrap();
        assert!(
            project.root.is_absolute(),
            "root must be absolute, got {}",
            project.root.display()
        );
        assert_eq!(project.root, dir);
    }
}
