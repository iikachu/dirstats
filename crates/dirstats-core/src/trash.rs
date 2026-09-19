// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Moving entries to the platform trash (the Recycle Bin on Windows) and
//! putting them back. Needs the `trash` feature.
//!
//! - [`App::trash_selected`] and [`App::trash_node`] move an entry to the
//!   trash; the tree is not rescanned, and [`App::is_trashed`] says what
//!   is gone.
//! - [`App::can_put_back`] and [`App::put_back`] restore it, when the
//!   platform said where it went.
//!
//! [`App::check_removable`] refuses the scan root, drive roots, the home
//! folder and (on macOS) Time Machine backups before anything moves.
//!
//! Programs without an [`App`] use the path-based functions the methods
//! are built on: [`move_to_trash`] and [`put_back`], which refuse what
//! [`crate::check_removable`] refuses.

use crate::{App, NodeId};
use std::io;
use std::path::{Path, PathBuf};

impl App {
    /// Move the selected entry to the trash. The tree is not rescanned.
    pub fn trash_selected(&mut self) -> io::Result<()> {
        let id = self.selected().ok_or(io::ErrorKind::NotFound)?;
        self.trash_node(id)
    }

    /// Move `id` to the trash. The tree is not rescanned.
    ///
    /// On Windows the shell refuses, rather than permanently deleting,
    /// items too large for the Recycle Bin or on a drive without one; the
    /// error then names neither cause, so front ends offer
    /// `App::delete_node_permanently` (Windows and Linux, `delete`
    /// feature) as the next step.
    pub fn trash_node(&mut self, id: NodeId) -> io::Result<()> {
        let path = self.path_of(id).ok_or(io::ErrorKind::NotFound)?;
        self.check_removable(id)?;
        let location = move_unchecked(&path)?;
        self.trashed.insert(id, location);
        self.message = Some(format!("moved to {}: {}", NAME, path.display()));
        Ok(())
    }

    /// Move `id` back from the trash to where it was scanned. Fails with
    /// `Unsupported` when the trash location is unknown and `AlreadyExists`
    /// when something else now has the original path.
    pub fn put_back(&mut self, id: NodeId) -> io::Result<()> {
        let original = self.path_of(id).ok_or(io::ErrorKind::NotFound)?;
        let Some(Some(location)) = self.trashed.get(&id).cloned() else {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "trash location unknown"));
        };
        put_back(&location, &original)?;
        self.trashed.remove(&id);
        self.message = Some(format!("put back: {}", original.display()));
        Ok(())
    }
}

/// Move `path` to the trash, and return where it went when the platform
/// says: the trashed item's path on macOS, its trash entry's id on Windows,
/// Linux and BSD. Keep it for [`put_back`].
///
/// Refuses what [`crate::check_removable`] refuses. On Windows the shell
/// refuses, rather than permanently deleting, items too large for the
/// Recycle Bin or on a drive without one.
pub fn move_to_trash(path: &Path) -> io::Result<Option<PathBuf>> {
    crate::check_removable(path)?;
    move_unchecked(path)
}

/// [`move_to_trash`] without the path checks, for [`App`], whose own
/// checks also know the scan root and the scanned tree.
fn move_unchecked(path: &Path) -> io::Result<Option<PathBuf>> {
    dataless::materialising(|| trash_backend::trash(path))
}

/// Move a trashed item from `location`, as [`move_to_trash`] returned it,
/// back to `original`. Refuses when something else is at `original` now.
pub fn put_back(location: &Path, original: &Path) -> io::Result<()> {
    if original.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "something else is at the original path"));
    }
    dataless::materialising(|| trash_backend::put_back(location, original))
}

/// Actions that move or open files must be allowed to fetch an evicted
/// iCloud item, while the scanner keeps the process from ever doing so
/// (see the scan crate). A thread-scoped policy overrides the process
/// one, so the acting thread turns fetching on just for the operation.
#[cfg(target_os = "macos")]
mod dataless {
    use std::ffi::c_int;
    // From <sys/resource.h>; not in the libc crate this project pins.
    const IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES: c_int = 3;
    const IOPOL_SCOPE_THREAD: c_int = 1;
    const IOPOL_MATERIALIZE_DATALESS_FILES_DEFAULT: c_int = 0;
    const IOPOL_MATERIALIZE_DATALESS_FILES_ON: c_int = 2;

    unsafe extern "C" {
        fn setiopolicy_np(iotype: c_int, scope: c_int, policy: c_int) -> c_int;
    }

    pub fn materialising<T>(f: impl FnOnce() -> T) -> T {
        // SAFETY: plain policy calls on the current thread; failure only
        // means the policy is unsupported and the operation runs as is.
        unsafe { setiopolicy_np(IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES, IOPOL_SCOPE_THREAD, IOPOL_MATERIALIZE_DATALESS_FILES_ON) };
        let result = f();
        unsafe { setiopolicy_np(IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES, IOPOL_SCOPE_THREAD, IOPOL_MATERIALIZE_DATALESS_FILES_DEFAULT) };
        result
    }
}

#[cfg(not(target_os = "macos"))]
mod dataless {
    pub fn materialising<T>(f: impl FnOnce() -> T) -> T {
        f()
    }
}

/// What the platform calls its trash, for messages.
pub const NAME: &str = if cfg!(windows) { "Recycle Bin" } else { "Trash" };

/// Move `path` to the trash and return where it went, when the platform
/// reports it. On macOS the direct NSFileManager call is used rather than
/// scripting Finder, so no Automation permission is requested, and the
/// resulting URL is kept for Put Back.
#[cfg(target_os = "macos")]
fn platform_trash(path: &Path) -> io::Result<Option<PathBuf>> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let Some(utf8) = path.to_str() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "path is not UTF-8"));
    };
    let url = NSURL::fileURLWithPath(&NSString::from_str(utf8));
    let mut resulting = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
        .map_err(|err| io::Error::other(err.localizedDescription().to_string()))?;
    Ok(resulting.and_then(|url| url.path()).map(|p| PathBuf::from(p.to_string())))
}

/// Elsewhere (the Windows Recycle Bin, the freedesktop trash on Linux and
/// BSD) the trash entry is looked up after the move, the most recent one
/// from `path`, and its id kept for Put Back.
#[cfg(not(target_os = "macos"))]
fn platform_trash(path: &Path) -> io::Result<Option<PathBuf>> {
    // Paths are compared canonical, as the scan may name the folder by
    // its short (8.3) name or a symlink and the trash by the real one.
    let parent = path.parent().and_then(|p| p.canonicalize().ok());
    trash::delete(path).map_err(io::Error::other)?;
    let (Some(parent), Some(name)) = (parent, path.file_name()) else { return Ok(None) };
    // The item is in the bin whether or not it can be found there again.
    let Ok(items) = trash::os_limited::list() else { return Ok(None) };
    Ok(items
        .into_iter()
        .filter(|item| item.name == name && item.original_parent.canonicalize().is_ok_and(|p| p == parent))
        .max_by_key(|item| item.time_deleted)
        .map(|item| PathBuf::from(item.id)))
}

/// Move a trashed item from `location`, as [`platform_trash`] reported it,
/// back to `original`.
#[cfg(target_os = "macos")]
fn platform_put_back(location: &Path, original: &Path) -> io::Result<()> {
    std::fs::rename(location, original)
}

/// Restored through the trash rather than renamed, so its record of the
/// item (the Recycle Bin's `$I` file, the `.trashinfo`) goes with it.
#[cfg(not(target_os = "macos"))]
fn platform_put_back(location: &Path, original: &Path) -> io::Result<()> {
    let items = trash::os_limited::list().map_err(io::Error::other)?;
    let Some(item) = items.into_iter().find(|item| Path::new(&item.id) == location) else {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("no longer in the {NAME}")));
    };
    trash::os_limited::restore_all([item]).map_err(io::Error::other)?;
    if original.exists() { Ok(()) } else { Err(io::Error::other("restored somewhere else")) }
}

/// The platform trash, which tests swap for a folder of their own so the
/// App's trash and Put Back bookkeeping runs without touching the real one.
mod trash_backend {
    use std::io;
    use std::path::{Path, PathBuf};

    #[cfg(test)]
    pub use fake::Fake;

    pub fn trash(path: &Path) -> io::Result<Option<PathBuf>> {
        #[cfg(test)]
        if let Some(result) = fake::trash(path) {
            return result;
        }
        super::platform_trash(path)
    }

    pub fn put_back(location: &Path, original: &Path) -> io::Result<()> {
        #[cfg(test)]
        if let Some(result) = fake::put_back(location, original) {
            return result;
        }
        super::platform_put_back(location, original)
    }

    #[cfg(test)]
    mod fake {
        use std::cell::RefCell;
        use std::io;
        use std::path::{Path, PathBuf};

        /// A trash for the current thread: items are renamed into `dir`.
        /// With `reports_location` off it behaves like a platform whose
        /// trash cannot say where an item went.
        pub struct Fake {
            pub dir: tempfile::TempDir,
            pub reports_location: bool,
            count: usize,
        }

        thread_local!(static FAKE: RefCell<Option<Fake>> = const { RefCell::new(None) });

        impl Fake {
            /// Install a fake trash for this thread; tests run one per thread.
            pub fn install(reports_location: bool) -> PathBuf {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().to_path_buf();
                FAKE.with(|f| *f.borrow_mut() = Some(Fake { dir, reports_location, count: 0 }));
                path
            }
        }

        pub fn trash(path: &Path) -> Option<io::Result<Option<PathBuf>>> {
            FAKE.with(|f| {
                let mut f = f.borrow_mut();
                let fake = f.as_mut()?;
                fake.count += 1;
                let to = fake.dir.path().join(format!("{}-{}", fake.count, path.file_name().unwrap().to_string_lossy()));
                Some(std::fs::rename(path, &to).map(|()| fake.reports_location.then_some(to)))
            })
        }

        pub fn put_back(location: &Path, original: &Path) -> Option<io::Result<()>> {
            FAKE.with(|f| f.borrow().as_ref().map(|_| std::fs::rename(location, original)))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::app_with_scan;
    use crate::{App, NodeId};
    use std::{fs, io};

    /// Moves a real temporary file to the system trash, and back again; run explicitly with
    /// `cargo test -p dirstats-core --features trash -- --ignored`.
    #[test]
    #[ignore]
    fn trashes_a_file_without_prompting() {
        let mut app = app_with_scan();
        let small = app.entries()[1];
        let path = app.path_of(small).unwrap();
        assert!(path.exists());
        app.trash_node(small).unwrap();
        assert!(!path.exists(), "file should have moved to the trash");
        assert!(app.is_trashed(small));
        assert!(app.message.as_deref().unwrap().starts_with("moved to "));
        assert!(app.can_put_back(small), "trash location not found");
        app.put_back(small).unwrap();
        assert!(path.exists(), "file should be back");
        assert!(!app.is_trashed(small));
    }

    mod put_back {
        use super::*;
        use crate::trash::trash_backend::Fake;

        fn named(app: &App, name: &str) -> NodeId {
            app.entries().iter().copied().find(|&id| app.tree.as_ref().unwrap().node(id).name.to_string_lossy() == name).unwrap()
        }

        #[test]
        fn trashes_and_puts_back_a_file() {
            let trash = Fake::install(true);
            let mut app = app_with_scan();
            let small = named(&app, "small");
            let path = app.path_of(small).unwrap();

            app.trash_node(small).unwrap();
            assert!(!path.exists());
            assert!(app.is_trashed(small) && app.can_put_back(small));
            assert_eq!(fs::read_dir(&trash).unwrap().count(), 1);
            assert!(app.message.as_deref().unwrap().starts_with("moved to "));

            app.put_back(small).unwrap();
            assert_eq!(fs::read(&path).unwrap(), vec![0u8; 10]);
            assert!(!app.is_trashed(small) && !app.can_put_back(small));
            assert_eq!(fs::read_dir(&trash).unwrap().count(), 0);
            assert_eq!(app.message.as_deref(), Some(&*format!("put back: {}", path.display())));
        }

        #[test]
        fn trashes_and_puts_back_a_folder_with_its_contents() {
            Fake::install(true);
            let mut app = app_with_scan();
            let sub = named(&app, "sub");
            let big = app.tree.as_ref().unwrap().children(sub)[0];
            let path = app.path_of(sub).unwrap();

            app.trash_node(sub).unwrap();
            assert!(!path.exists());
            assert!(app.is_trashed(big), "descendants count as trashed");
            assert!(!app.can_put_back(big), "only the trashed node itself goes back");

            app.put_back(sub).unwrap();
            assert_eq!(fs::read(path.join("big")).unwrap().len(), 5000);
            assert!(!app.is_trashed(sub) && !app.is_trashed(big));
        }

        #[test]
        fn refuses_when_the_original_path_is_taken() {
            let trash = Fake::install(true);
            let mut app = app_with_scan();
            let small = named(&app, "small");
            let path = app.path_of(small).unwrap();
            app.trash_node(small).unwrap();
            fs::write(&path, b"new").unwrap();

            let err = app.put_back(small).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
            assert_eq!(fs::read(&path).unwrap(), b"new", "the newcomer is left alone");
            assert!(app.can_put_back(small), "still offered once the path is free");
            assert_eq!(fs::read_dir(&trash).unwrap().count(), 1);
        }

        #[test]
        fn unknown_location_is_trashed_but_not_put_back() {
            Fake::install(false);
            let mut app = app_with_scan();
            let small = named(&app, "small");
            app.trash_node(small).unwrap();
            assert!(app.is_trashed(small));
            assert!(!app.can_put_back(small));
            assert_eq!(app.put_back(small).unwrap_err().kind(), io::ErrorKind::Unsupported);
        }

        #[test]
        fn never_trashed_is_not_put_back() {
            Fake::install(true);
            let mut app = app_with_scan();
            let small = named(&app, "small");
            assert!(!app.can_put_back(small));
            assert_eq!(app.put_back(small).unwrap_err().kind(), io::ErrorKind::Unsupported);
            assert!(app.path_of(small).unwrap().exists());
        }

        #[test]
        fn failed_put_back_keeps_the_item_trashed() {
            let trash = Fake::install(true);
            let mut app = app_with_scan();
            let small = named(&app, "small");
            app.trash_node(small).unwrap();
            // Emptied from the trash behind the app's back.
            for entry in fs::read_dir(&trash).unwrap() {
                fs::remove_file(entry.unwrap().path()).unwrap();
            }
            assert!(app.put_back(small).is_err());
            assert!(app.is_trashed(small));
        }

        #[test]
        fn refuses_to_trash_the_scan_root() {
            let trash = Fake::install(true);
            let mut app = app_with_scan();
            let root = app.tree.as_ref().unwrap().root();
            assert_eq!(app.trash_node(root).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
            assert!(!app.is_trashed(root));
            assert_eq!(fs::read_dir(&trash).unwrap().count(), 0);
        }

        #[test]
        fn path_functions_trash_and_put_back_without_an_app() {
            let trash = Fake::install(true);
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("f");
            fs::write(&file, b"x").unwrap();

            let location = crate::trash::move_to_trash(&file).unwrap().expect("fake reports where it went");
            assert!(!file.exists());
            assert_eq!(fs::read_dir(&trash).unwrap().count(), 1);

            fs::write(&file, b"new").unwrap();
            assert_eq!(crate::trash::put_back(&location, &file).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
            fs::remove_file(&file).unwrap();
            crate::trash::put_back(&location, &file).unwrap();
            assert_eq!(fs::read(&file).unwrap(), b"x");
        }

        #[test]
        fn path_functions_refuse_the_home_folder() {
            let trash = Fake::install(true);
            let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap();
            let err = crate::trash::move_to_trash(std::path::Path::new(&home)).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
            assert_eq!(fs::read_dir(&trash).unwrap().count(), 0);
        }
    }
}
