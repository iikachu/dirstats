// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

//! iCloud Drive items on macOS: whether a path is one, and evicting its
//! local copy so the space comes back without deleting anything.

use std::io;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudStatus {
    /// Not managed by iCloud Drive.
    Local,
    /// Synced and present on disk; its download can be removed.
    Downloaded,
    /// Synced but only a placeholder on disk.
    Evicted,
}

#[cfg(all(target_os = "macos", feature = "icloud"))]
fn url(path: &Path) -> io::Result<objc2::rc::Retained<objc2_foundation::NSURL>> {
    let Some(utf8) = path.to_str() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "path is not UTF-8"));
    };
    Ok(objc2_foundation::NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(utf8)))
}

/// What iCloud knows about `path`, and whether it is on disk.
///
/// Membership is the system's own `isUbiquitousItem` answer, which also
/// covers items excluded from sync inside a synced folder (`.DS_Store`,
/// `.nosync` names, the file-provider ignore attribute). For a file the
/// download state is the kernel's dataless flag; a directory reads as
/// evicted when nothing below it occupies disk, from the scanned sizes.
/// Both lookups are a few microseconds, so front ends ask per displayed
/// node and cache per tree. Errors read as "local", so the menu never
/// fails on odd paths.
#[cfg(all(target_os = "macos", feature = "icloud"))]
#[must_use]
pub fn status(path: &Path, is_dir: bool, apparent: u64, allocated: u64) -> CloudStatus {
    use objc2_foundation::{NSNumber, NSURLIsUbiquitousItemKey};
    let Ok(url) = url(path) else { return CloudStatus::Local };
    let mut value = None;
    // SAFETY: the key names a boolean resource, read into an NSNumber.
    if unsafe { url.getResourceValue_forKey_error(&mut value, NSURLIsUbiquitousItemKey) }.is_err() {
        return CloudStatus::Local;
    }
    let in_cloud = value.as_ref().and_then(|v| v.downcast_ref::<NSNumber>()).is_some_and(NSNumber::boolValue);
    if !in_cloud {
        return CloudStatus::Local;
    }
    if is_dir {
        return from_sizes(apparent, allocated);
    }
    if dataless(path).unwrap_or_else(|| from_sizes(apparent, allocated) == CloudStatus::Evicted) {
        CloudStatus::Evicted
    } else {
        CloudStatus::Downloaded
    }
}

/// The kernel's dataless flag on `path`; `None` when it cannot be read.
#[cfg(all(target_os = "macos", feature = "icloud"))]
fn dataless(path: &Path) -> Option<bool> {
    use std::os::unix::ffi::OsStrExt;
    // From <sys/stat.h>: SF_DATALESS marks a placeholder whose data lives elsewhere.
    const SF_DATALESS: u32 = 0x4000_0000;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    (unsafe { libc::lstat(c.as_ptr(), &mut st) } == 0).then_some(st.st_flags & SF_DATALESS != 0)
}

/// Download state from scanned sizes, used for directories: their sizes
/// are the totals below, so a folder with nothing downloaded reads as
/// evicted while one with any local data does not.
#[must_use]
pub fn from_sizes(apparent: u64, allocated: u64) -> CloudStatus {
    if apparent > 0 && allocated == 0 { CloudStatus::Evicted } else { CloudStatus::Downloaded }
}

/// Remove the local copy of a synced item, keeping it in iCloud. For a
/// folder, every file below it is evicted.
#[cfg(all(target_os = "macos", feature = "icloud"))]
pub fn evict(path: &Path) -> io::Result<()> {
    objc2_foundation::NSFileManager::defaultManager()
        .evictUbiquitousItemAtURL_error(&*url(path)?)
        .map_err(|err| io::Error::other(err.localizedDescription().to_string()))
}

#[cfg(not(all(target_os = "macos", feature = "icloud")))]
#[must_use]
pub fn status(_path: &Path, _is_dir: bool, _apparent: u64, _allocated: u64) -> CloudStatus {
    CloudStatus::Local
}

#[cfg(not(all(target_os = "macos", feature = "icloud")))]
pub fn evict(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "no cloud storage on this platform"))
}

#[cfg(feature = "icloud")]
impl crate::App {
    /// Remove the local copy of a synced iCloud item, keeping it in the cloud.
    pub fn evict_node(&mut self, id: crate::NodeId) -> io::Result<()> {
        let path = self.path_of(id).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such entry"))?;
        evict(&path)?;
        self.evicted.insert(id);
        self.message = Some(format!("removed download: {} (rescan to update sizes)", path.display()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_files_are_local() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(status(&file, false, 1, 4096), CloudStatus::Local);
        assert!(evict(&file).is_err(), "evicting a local file is refused");
    }

    #[cfg(all(target_os = "macos", feature = "icloud"))]
    #[test]
    fn plain_files_are_not_dataless() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(dataless(&file), Some(false));
        assert_eq!(dataless(&dir.path().join("missing")), None);
    }

    #[test]
    fn directory_state_comes_from_sizes() {
        assert_eq!(from_sizes(10_000, 0), CloudStatus::Evicted);
        assert_eq!(from_sizes(10_000, 12_288), CloudStatus::Downloaded);
        assert_eq!(from_sizes(0, 0), CloudStatus::Downloaded, "nothing to fetch when empty");
    }
}
