//! All filesystem operations for the app go through here: locating the
//! config directory, and generic read/write/remove of files as strings.

use std::path::{Path, PathBuf};

use crate::bail;

const CONFIG_SUBDIR: &str = "hypr";

pub struct FileOps;

impl FileOps {
    /// Directory holding all of this app's config/state files, e.g. `~/.config/hypr`.
    pub fn config_dir() -> PathBuf {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg).join(CONFIG_SUBDIR);
        }

        let Some(home) = std::env::var_os("HOME") else {
            bail!("HOME is not set");
        };

        PathBuf::from(home).join(".config").join(CONFIG_SUBDIR)
    }

    /// Make sure the config directory exists, creating it if necessary.
    pub fn ensure_config_dir() -> PathBuf {
        let directory = Self::config_dir();
        if let Err(error) = std::fs::create_dir_all(&directory) {
            bail!("creating config directory {}: {error}", directory.display());
        }
        directory
    }

    /// Read a file's contents as a UTF-8 string.
    pub fn read_to_string(path: &Path) -> String {
        match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) => bail!("reading file {}: {error}", path.display()),
        }
    }

    /// Write a string to a file, creating parent directories as needed.
    pub fn write_string(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                bail!("creating directory {}: {error}", parent.display());
            }
        }

        if let Err(error) = std::fs::write(path, contents) {
            bail!("writing file {}: {error}", path.display());
        }
    }

    /// Remove a file. A file that is already gone is not an error.
    pub fn remove_file(path: &Path) {
        if let Err(error) = std::fs::remove_file(path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                bail!("removing file {}: {error}", path.display());
            }
        }
    }
}
