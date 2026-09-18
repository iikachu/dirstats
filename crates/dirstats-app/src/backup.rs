// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

//! Time Machine backups on macOS: whether a path lies inside one, so
//! front ends can point at Time Machine instead of offering the trash.
//!
//! Backups are managed by Time Machine itself (its settings, or
//! `tmutil`); moving pieces of one to the trash either fails or
//! leaves a backup that no longer restores. The checks run on any
//! platform, since a backup disk can be read from anywhere, but only the
//! volume probe touches the disk.

use std::path::{Component, Path};

/// Where Time Machine is managed, for messages.
pub const MANAGED_ELSEWHERE: &str =
    "Managed by Time Machine. Remove old backups in Time Machine settings or with `tmutil`.";

/// Whether `path` is inside a Time Machine backup, from its components
/// alone: the HFS+ backup store (`Backups.backupdb`) or the mount point
/// macOS uses to browse APFS backup snapshots (`/Volumes/.timemachine`).
/// No disk access, so front ends may ask per displayed row.
#[must_use]
pub fn in_backup_path(path: &Path) -> bool {
    if path.starts_with("/Volumes/.timemachine") {
        return true;
    }
    path.components().any(|c| matches!(c, Component::Normal(name) if name == "Backups.backupdb"))
}

/// Whether the volume holding `root` is a Time Machine destination, from
/// the entries at its top level: APFS backups keep one
/// `<date>.backup`, `.previous` or `.interrupted` entry per backup there,
/// HFS+ backups a `Backups.backupdb` folder. Reads one directory, so call
/// it once per scan, not per node.
#[must_use]
pub fn is_backup_volume(root: &Path) -> bool {
    if in_backup_path(root) {
        return true;
    }
    let Some(top) = volume_root(root) else { return false };
    let Ok(entries) = std::fs::read_dir(top) else { return false };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        name == "Backups.backupdb" || [".backup", ".previous", ".interrupted"].iter().any(|ext| is_dated(&name, ext))
    })
}

/// `YYYY-MM-DD-HHMMSS<ext>`, the name Time Machine gives each backup.
fn is_dated(name: &str, ext: &str) -> bool {
    let Some(stem) = name.strip_suffix(ext) else { return false };
    let b = stem.as_bytes();
    b.len() == 17
        && b.iter().enumerate().all(|(i, c)| if matches!(i, 4 | 7 | 10) { *c == b'-' } else { c.is_ascii_digit() })
}

/// Mount point of the volume holding `path`.
#[cfg(unix)]
fn volume_root(path: &Path) -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statfs(c.as_ptr(), &mut fs) } != 0 {
            return None;
        }
        let mount = unsafe { std::ffi::CStr::from_ptr(fs.f_mntonname.as_ptr()) };
        Some(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(mount.to_bytes())))
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Walk up while the device stays the same.
        use std::os::unix::fs::MetadataExt;
        let dev = std::fs::metadata(path).ok()?.dev();
        let mut top = path.to_path_buf();
        while let Some(parent) = top.parent() {
            if std::fs::metadata(parent).ok()?.dev() != dev {
                break;
            }
            top = parent.to_path_buf();
        }
        Some(top)
    }
}

#[cfg(not(unix))]
fn volume_root(path: &Path) -> Option<std::path::PathBuf> {
    path.ancestors().last().map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_paths() {
        assert!(in_backup_path(Path::new("/Volumes/TM/Backups.backupdb/Mac/Latest/Macintosh HD/Users")));
        assert!(in_backup_path(Path::new("/Volumes/.timemachine/ABC/2026-09-01-120000.backup/2026-09-01-120000.backup")));
        assert!(!in_backup_path(Path::new("/Users/me/Backups")));
        assert!(!in_backup_path(Path::new("/Volumes/.timemachinex/a")));
    }

    #[test]
    fn dated_names() {
        assert!(is_dated("2026-09-01-120000.backup", ".backup"));
        assert!(is_dated("2026-09-01-120000.previous", ".previous"));
        assert!(!is_dated("2026-09-01.backup", ".backup"));
        assert!(!is_dated("notes.backup", ".backup"));
    }

    #[test]
    fn ordinary_folders_are_not_backups() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_backup_volume(dir.path()));
        assert!(is_backup_volume(&dir.path().join("Backups.backupdb")));
    }
}
