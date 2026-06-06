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
                let root = canonicalize_dir(dir)?;
                let config_path = root.join(CONFIG_FILE);
                return Ok(Project { root, config_path });
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
    /// The file's parent directory becomes the project root. Both `root` and
    /// `config_path` are always absolute (canonicalized), even when `path` is
    /// a bare filename like `ubproject.toml`.
    pub fn from_config_path(path: &Path) -> Result<Project, Error> {
        if !path.is_file() {
            return Err(Error::Config {
                message: format!("config file not found: {}", path.display()),
            });
        }
        // Canonicalize the config file itself so `config_path` is always absolute,
        // matching what `discover` stores.
        let config_path = path.canonicalize().map_err(|e| Error::Config {
            message: format!("cannot resolve config path {}: {e}", path.display()),
        })?;
        // The canonical parent is the project root (guaranteed non-empty after
        // canonicalize, since the file exists).
        let root = config_path
            .parent()
            .expect("canonical file path always has a parent")
            .to_path_buf();
        Ok(Project { root, config_path })
    }
}

/// Canonicalize `dir` via `fs::canonicalize` (resolves symlinks; the path must exist).
///
/// Unlike `resolve_against_root` in `config.rs`, this hits the filesystem and is
/// fallible — use it only for paths that are known to exist at call time.
fn canonicalize_dir(dir: &Path) -> Result<PathBuf, Error> {
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

    /// `from_config_path` must store an absolute `config_path`, even when
    /// the caller passes a bare filename like `ubproject.toml`.
    #[test]
    fn from_config_path_stores_absolute_config_path() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        std::fs::write(dir.join(CONFIG_FILE), "").unwrap();

        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let result = Project::from_config_path(Path::new(CONFIG_FILE));
        std::env::set_current_dir(&original).unwrap();

        let project = result.unwrap();
        assert!(
            project.config_path.is_absolute(),
            "config_path must be absolute, got {}",
            project.config_path.display()
        );
        assert_eq!(project.config_path, dir.join(CONFIG_FILE));
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
