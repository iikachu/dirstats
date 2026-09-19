// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

//! Places worth offering to scan when none was given: the home folder,
//! the mounted disks, and the root of the filesystem.
//!
//! macOS lists the boot volume and whatever is mounted under `/Volumes`,
//! skipping the system's own hidden mounts (Preboot, VM, the Data volume
//! and friends) that would only duplicate the root, as Disk Inventory X
//! does. Linux lists mounts backed by a block device, a network share,
//! ZFS or FUSE.
//! Windows lists every drive letter, as WinDirStat does.

use std::path::PathBuf;

/// What sort of place a [`Location`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The user's home folder.
    Home,
    /// A mounted disk, drive or share.
    Volume,
    /// The root of the whole filesystem.
    Root,
}

/// A place offered to scan, with its filesystem's size.
#[derive(Clone, Debug)]
pub struct Location {
    /// Label to show: "Home", "Root", a volume or drive name.
    pub name: String,
    /// Where to scan.
    pub path: PathBuf,
    /// What sort of place it is.
    pub kind: Kind,
    /// Capacity in bytes, when the filesystem reports it.
    pub total: Option<u64>,
    /// Bytes available to the user, when the filesystem reports it.
    pub free: Option<u64>,
}

/// Home first, then volumes, then the filesystem root unless a volume is
/// already mounted there (the boot volume on macOS). Windows has no
/// single root; its drives are the roots.
#[must_use]
pub fn list() -> Vec<Location> {
    let mut out = Vec::new();
    if let Some(home) = home() {
        let (total, free) = space(&home);
        out.push(Location { name: "Home".into(), path: home, kind: Kind::Home, total, free });
    }
    out.extend(volumes());
    #[cfg(not(windows))]
    {
        let root = PathBuf::from("/");
        if !out.iter().any(|l| l.path == root) {
            let (total, free) = space(&root);
            out.push(Location { name: "Root".into(), path: root, kind: Kind::Root, total, free });
        }
    }
    out
}

/// Whether this process may read the folders macOS guards with Full Disk
/// Access. `None` when the platform has no such gate or nothing to probe.
///
/// A process without it gets permission errors on folders like Mail and
/// Messages, and macOS asks per folder (Desktop, Documents, Downloads,
/// removable and network volumes) with a dialog that blocks the directory
/// read until it is answered. A scan started from Finder therefore looks
/// stalled while the dialog waits, often behind the window. The probe
/// itself never triggers a dialog: these folders are only ever granted
/// through System Settings.
#[must_use]
pub fn full_disk_access() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        let home = home()?;
        for guarded in ["Library/Mail", "Library/Messages", "Library/Safari"] {
            match std::fs::read_dir(home.join(guarded)) {
                Ok(_) => return Some(true),
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Some(false),
                Err(_) => {}
            }
        }
        None
    }
    #[cfg(not(target_os = "macos"))]
    None
}

/// Open the system's privacy settings at the Full Disk Access list.
#[cfg(target_os = "macos")]
pub fn open_full_disk_access_settings() -> std::io::Result<()> {
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
        .spawn()
        .map(drop)
}

/// `USERPROFILE` or else `HOME`, when it names an existing directory.
fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).filter(|p| p.is_dir())
}

/// `(total, free)` bytes of the filesystem holding `path`.
#[cfg(unix)]
fn space(path: &std::path::Path) -> (Option<u64>, Option<u64>) {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return (None, None) };
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return (None, None);
    }
    let frag = s.f_frsize as u64;
    (Some(s.f_blocks as u64 * frag), Some(s.f_bavail as u64 * frag))
}

#[cfg(target_os = "macos")]
fn volumes() -> Vec<Location> {
    use std::ffi::CStr;
    let mut out = Vec::new();
    // The boot volume appears in /Volumes as a symlink to "/"; that entry
    // carries its user-visible name.
    let boot_name = std::fs::read_dir("/Volumes")
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .find(|e| std::fs::canonicalize(e.path()).is_ok_and(|p| p == std::path::Path::new("/")))
        .map(|e| e.file_name().to_string_lossy().into_owned());
    unsafe {
        let mut mounts: *mut libc::statfs = std::ptr::null_mut();
        let count = libc::getmntinfo(&mut mounts, libc::MNT_NOWAIT);
        for i in 0..count.max(0) as usize {
            let m = &*mounts.add(i);
            if m.f_flags & libc::MNT_DONTBROWSE as u32 != 0 {
                continue;
            }
            let mount = CStr::from_ptr(m.f_mntonname.as_ptr()).to_string_lossy().into_owned();
            let is_root = mount == "/";
            // Everything else (/System/Volumes/*, /private/var/vm, /dev …)
            // is a piece of the boot disk or not a disk at all.
            if !is_root && !mount.starts_with("/Volumes/") {
                continue;
            }
            let name = if is_root {
                boot_name.clone().unwrap_or_else(|| "Macintosh HD".into())
            } else {
                mount.rsplit('/').next().unwrap_or(&mount).to_string()
            };
            let block = u64::from(m.f_bsize);
            out.push(Location {
                name,
                path: PathBuf::from(mount),
                kind: Kind::Volume,
                total: Some(m.f_blocks * block),
                free: Some(m.f_bavail * block),
            });
        }
    }
    // Boot volume first, then the rest by name.
    out.sort_by(|a, b| (a.path != std::path::Path::new("/")).cmp(&(b.path != std::path::Path::new("/"))).then_with(|| a.name.cmp(&b.name)));
    out
}

#[cfg(all(unix, not(target_os = "macos")))]
fn volumes() -> Vec<Location> {
    let mut out = Vec::new();
    let mut seen_devices = std::collections::HashSet::new();
    let Ok(mounts) = std::fs::read_to_string("/proc/self/mounts") else { return out };
    for line in mounts.lines() {
        let mut fields = line.split_whitespace();
        let (Some(device), Some(mount), Some(fstype)) = (fields.next(), fields.next(), fields.next()) else { continue };
        let network = matches!(fstype, "nfs" | "nfs4" | "cifs" | "smb3" | "zfs") || fstype.starts_with("fuse.");
        if !(device.starts_with("/dev/") || network) || fstype == "squashfs" || mount == "/" {
            continue;
        }
        // Bind mounts and subvolumes share a device; list each disk once.
        if !seen_devices.insert(device.to_string()) {
            continue;
        }
        let mount = unescape_mount(mount);
        let name = mount.rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or(&mount).to_string();
        let path = PathBuf::from(&mount);
        let (total, free) = space(&path);
        out.push(Location { name, path, kind: Kind::Volume, total, free });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// `/proc/mounts` escapes spaces and friends as octal (`\040`).
#[cfg(all(unix, not(target_os = "macos")))]
fn unescape_mount(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() && bytes[i + 1..i + 4].iter().all(u8::is_ascii_digit) {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 4], 8) {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(windows)]
fn space(path: &std::path::Path) -> (Option<u64>, Option<u64>) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let (mut free, mut total, mut total_free) = (0u64, 0u64, 0u64);
    if unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, &mut total_free) } == 0 {
        return (None, None);
    }
    (Some(total), Some(free))
}

#[cfg(windows)]
fn volumes() -> Vec<Location> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
    use windows_sys::Win32::System::WindowsProgramming::{DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE};
    let mut out = Vec::new();
    let mask = unsafe { GetLogicalDrives() };
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = char::from(b'A' + i as u8);
        let root = format!("{letter}:\\");
        let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let kind = unsafe { GetDriveTypeW(wide.as_ptr()) };
        if !matches!(kind, DRIVE_FIXED | DRIVE_REMOVABLE | DRIVE_REMOTE | DRIVE_CDROM | DRIVE_RAMDISK) {
            continue;
        }
        let mut label = [0u16; 261];
        let ok = unsafe {
            GetVolumeInformationW(
                wide.as_ptr(),
                label.as_mut_ptr(),
                label.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            )
        };
        let label = if ok == 0 {
            String::new()
        } else {
            let end = label.iter().position(|&c| c == 0).unwrap_or(label.len());
            String::from_utf16_lossy(&label[..end])
        };
        let label = if label.is_empty() {
            match kind {
                DRIVE_REMOVABLE => "Removable Disk".to_string(),
                DRIVE_REMOTE => "Network Drive".to_string(),
                DRIVE_CDROM => "CD Drive".to_string(),
                _ => "Local Disk".to_string(),
            }
        } else {
            label
        };
        let path = PathBuf::from(&root);
        let (total, free) = space(&path);
        out.push(Location { name: format!("{label} ({letter}:)"), path, kind: Kind::Volume, total, free });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_home_and_volumes() {
        let locations = list();
        assert!(locations.iter().any(|l| l.kind == Kind::Home), "home folder listed");
        #[cfg(not(windows))]
        assert!(locations.iter().any(|l| l.path == std::path::Path::new("/")), "the root is reachable");
        let mut paths: Vec<_> = locations.iter().map(|l| &l.path).collect();
        paths.dedup();
        assert_eq!(paths.len(), locations.len(), "no duplicate paths");
        for l in &locations {
            assert!(l.path.is_dir(), "{} exists", l.path.display());
            assert!(!l.name.is_empty());
        }
    }

    #[test]
    fn full_disk_access_probe_is_quiet() {
        // Only asserts it answers; the value depends on the environment.
        let _ = full_disk_access();
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn unescapes_octal() {
        assert_eq!(unescape_mount("/media/My\\040Disk"), "/media/My Disk");
    }
}
