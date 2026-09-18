// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

//! Shared by the elevated MFT tests and the MFT bench: a generated tree to
//! scan, and a way to scan it cold.

#![allow(dead_code)]

use dirstats_scan::{NodeId, Tree};
use std::fs;
use std::path::{Path, PathBuf};

/// Directory name of the fixture below a volume root.
pub const FIXTURE: &str = "dirstats-mft-fixture";

/// Create `root\dirstats-mft-fixture` with `files` files spread over
/// directories 3 levels deep, unless it already holds that many. Sizes mix
/// records small enough to live inside the MFT with ones that need clusters.
pub fn fixture(root: &Path, files: usize) -> PathBuf {
    let dir = root.join(FIXTURE);
    let stamp = dir.join(format!("complete-{files}"));
    if stamp.exists() {
        return dir;
    }
    let _ = fs::remove_dir_all(&dir);
    const PER_DIR: usize = 100;
    for i in 0..files.div_ceil(PER_DIR) {
        let sub = dir.join(format!("a{}", i % 10)).join(format!("b{}", i / 10 % 10)).join(format!("c{i}"));
        fs::create_dir_all(&sub).unwrap();
        for j in 0..PER_DIR.min(files - i * PER_DIR) {
            let len = [0, 100, 700, 5_000, 70_000][j % 5];
            fs::write(sub.join(format!("f{j}")), vec![b'x'; len]).unwrap();
        }
    }
    fs::write(&stamp, b"").unwrap();
    dir
}

/// The node at `path` (relative to the tree's root), found by name.
pub fn find(tree: &Tree, path: &Path) -> Option<NodeId> {
    path.components().try_fold(tree.root(), |id, part| {
        tree.children(id).iter().copied().find(|&c| *tree.node(c).name == *part.as_os_str())
    })
}

/// Empty Windows' standby list so the next scan reads file data and
/// metadata from disk. Needs administrator rights.
#[cfg(windows)]
pub fn purge_standby_list() {
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LUID};
    use windows_sys::Win32::Security::{
        AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtSetSystemInformation(class: i32, info: *const c_void, len: u32) -> i32;
    }
    const SYSTEM_MEMORY_LIST_INFORMATION: i32 = 80;
    const MEMORY_FLUSH_MODIFIED_LIST: i32 = 3;
    const MEMORY_PURGE_STANDBY_LIST: i32 = 4;

    // SAFETY: plain Win32 calls with valid, correctly sized arguments.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        assert!(OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &mut token) != 0);
        let name: Vec<u16> = "SeProfileSingleProcessPrivilege\0".encode_utf16().collect();
        let mut luid = LUID { LowPart: 0, HighPart: 0 };
        assert!(LookupPrivilegeValueW(std::ptr::null(), name.as_ptr(), &mut luid) != 0);
        let privileges = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
        };
        let adjusted = AdjustTokenPrivileges(token, 0, &privileges, 0, std::ptr::null_mut(), std::ptr::null_mut());
        CloseHandle(token);
        assert!(adjusted != 0, "cannot enable SeProfileSingleProcessPrivilege (elevated?)");
        for command in [MEMORY_FLUSH_MODIFIED_LIST, MEMORY_PURGE_STANDBY_LIST] {
            let status = NtSetSystemInformation(SYSTEM_MEMORY_LIST_INFORMATION, (&raw const command).cast(), 4);
            assert!(status >= 0, "NtSetSystemInformation({command}) failed: {status:#x}");
        }
    }
}
