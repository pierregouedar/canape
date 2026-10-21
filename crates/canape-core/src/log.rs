//! Minimal append-only log files, without a logging framework.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// `%ProgramData%\Canape` for the service, `%LOCALAPPDATA%\Canape` for the
/// shell; the temp dir off Windows.
pub fn dir(windows_env_var: &str) -> PathBuf {
    std::env::var_os(windows_env_var)
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Canape")
}

/// Appends `<unix seconds> <message>` to `path`; logging never fails the caller.
pub fn append(path: &Path, message: &str) {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{seconds} {message}");
    }
}
