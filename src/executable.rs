use std::env;
use std::path::{Path, PathBuf};

use crate::config::config_var;

pub(crate) fn resolve_herdr_bin() -> String {
    config_var("HERDR_BIN_PATH")
        .or_else(|| find_executable("herdr", herdr_candidate_paths()))
        .unwrap_or_else(|| "herdr".to_string())
}

pub(crate) fn find_executable(name: &str, candidate_paths: Vec<PathBuf>) -> Option<String> {
    executable_in_path(name)
        .or_else(|| {
            candidate_paths
                .into_iter()
                .find(|path| is_executable_file(path))
        })
        .map(|path| path.to_string_lossy().into_owned())
}

pub(crate) fn executable_in_path(name: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|path| is_executable_file(path))
    })
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

#[cfg(unix)]
pub(crate) fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.is_file()
        && path
            .metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
pub(crate) fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

fn herdr_candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home_dir) = home_dir() {
        paths.push(home_dir.join(".local/bin/herdr"));
    }
    paths.push(PathBuf::from("/opt/homebrew/bin/herdr"));
    paths.push(PathBuf::from("/usr/local/bin/herdr"));
    paths
}
