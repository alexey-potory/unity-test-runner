use std::path::{Path, PathBuf};

/// Rust's `std::fs::canonicalize` returns Windows extended-length paths
/// such as `\\?\D:\repo` on Windows. Unity's command-line parser is not
/// reliable with those paths for `-projectPath`, `-testResults`, or `-logFile`,
/// so normalize them back to ordinary Win32 paths before passing them to Unity
/// and before printing JSON.
pub fn strip_windows_extended_prefix(value: &str) -> String {
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", rest)
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        value.to_string()
    }
}

pub fn normalize_for_unity(path: impl Into<PathBuf>) -> PathBuf {
    let path = path.into();
    PathBuf::from(strip_windows_extended_prefix(&path.to_string_lossy()))
}

pub fn path_to_json_string(path: &Path) -> String {
    strip_windows_extended_prefix(&path.to_string_lossy()).replace('/', &std::path::MAIN_SEPARATOR.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_drive_extended_prefix() {
        assert_eq!(
            strip_windows_extended_prefix(r"\\?\D:\Coding\Unity\Project"),
            r"D:\Coding\Unity\Project"
        );
    }

    #[test]
    fn strips_unc_extended_prefix() {
        assert_eq!(
            strip_windows_extended_prefix(r"\\?\UNC\server\share\Project"),
            r"\\server\share\Project"
        );
    }

    #[test]
    fn leaves_normal_path_unchanged() {
        assert_eq!(
            strip_windows_extended_prefix(r"D:\Coding\Unity\Project"),
            r"D:\Coding\Unity\Project"
        );
    }
}
