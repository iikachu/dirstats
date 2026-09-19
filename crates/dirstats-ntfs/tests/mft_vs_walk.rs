// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

//! The MFT reader and the walker agree on a real NTFS volume. Needs an
//! elevated prompt; the volume is `DIRSTATS_TEST_VOLUME` (default `C:\`).
//! The tests create a fixture of 20,000 files on that volume (see
//! `support::fixture`) and leave it there for the next run.
//!
//! ```text
//! cargo test -p dirstats-ntfs --test mft_vs_walk -- --ignored
//! ```
#![cfg(windows)]

mod support;

use dirstats_scan::{ScanOptions, SizeMetric, Tree};
use std::path::{Path, PathBuf};

fn volume() -> PathBuf {
    std::env::var_os("DIRSTATS_TEST_VOLUME").map_or_else(|| PathBuf::from(r"C:\"), PathBuf::from)
}

/// Scan the test volume from its table and by walking, after making sure
/// the fixture exists; `cold` empties the standby list before each scan.
fn scan_both(metric: SizeMetric, cold: bool) -> (Tree, Tree) {
    let root = volume();
    support::fixture(&root, 20_000);
    let options = ScanOptions { size_metric: metric, ..ScanOptions::default() };
    if cold {
        support::purge_standby_list();
    }
    let mft = dirstats_ntfs::scan_mft(&root, &options).expect("read the MFT (elevated? NTFS drive root?)");
    if cold {
        support::purge_standby_list();
    }
    let walk = dirstats_scan::scan(&root, &options).unwrap();
    (mft, walk)
}

/// Kind, counts and sizes of every directory in the fixture, and the names
/// of its children, match.
fn assert_fixture_agrees(mft: &Tree, walk: &Tree) {
    let fixture = Path::new(support::FIXTURE);
    let (m, w) = (support::find(mft, fixture).expect("fixture in MFT tree"), support::find(walk, fixture).unwrap());
    let mut pending = vec![(m, w, PathBuf::from(fixture))];
    while let Some((m, w, path)) = pending.pop() {
        let (mn, wn) = (mft.node(m), walk.node(w));
        let summary = |n: &dirstats_scan::Node| (n.kind, n.file_count, n.dir_count, n.apparent_size, n.allocated_size);
        assert_eq!(summary(mn), summary(wn), "{}: (kind, files, dirs, apparent, allocated)", path.display());
        let names = |t: &Tree, id| {
            let mut v: Vec<_> = t.children(id).iter().map(|&c| (t.node(c).name.clone(), c)).collect();
            v.sort();
            v
        };
        let (mc, wc) = (names(mft, m), names(walk, w));
        assert_eq!(mc.iter().map(|c| &c.0).collect::<Vec<_>>(), wc.iter().map(|c| &c.0).collect::<Vec<_>>(), "{}", path.display());
        for ((name, m), (_, w)) in mc.into_iter().zip(wc) {
            if mft.node(m).kind == dirstats_scan::Kind::Directory {
                pending.push((m, w, path.join(&*name)));
            }
        }
    }
}

#[test]
#[ignore = "needs an elevated prompt and an NTFS volume"]
fn fixture_matches_walk_apparent() {
    let (mft, walk) = scan_both(SizeMetric::Apparent, false);
    assert_fixture_agrees(&mft, &walk);
}

#[test]
#[ignore = "needs an elevated prompt and an NTFS volume"]
fn fixture_matches_walk_allocated_cold() {
    let (mft, walk) = scan_both(SizeMetric::Allocated, true);
    assert_fixture_agrees(&mft, &walk);
}

#[test]
#[ignore = "needs an elevated prompt and an NTFS volume"]
fn volume_totals_are_close() {
    let (mft, walk) = scan_both(SizeMetric::Allocated, false);
    let (m, w) = (mft.node(mft.root()), walk.node(walk.root()));
    // The table also sees system files and directories the walker cannot
    // open, so it may only find more, and not wildly more.
    assert!(m.file_count >= w.file_count * 99 / 100, "MFT {} files, walk {}", m.file_count, w.file_count);
    assert!(m.file_count <= w.file_count * 2, "MFT {} files, walk {}", m.file_count, w.file_count);
}

#[test]
#[ignore = "needs an elevated prompt and an NTFS volume"]
fn subfolders_and_non_roots_are_refused() {
    let root = volume();
    let sub = support::fixture(&root, 20_000);
    assert!(dirstats_ntfs::scan_mft(&sub, &ScanOptions::default()).is_err());
    // The public entry point walks such a root instead.
    let tree = dirstats_ntfs::scan(&sub, &ScanOptions::default()).unwrap();
    assert_eq!(tree.node(tree.root()).file_count, 20_000 + 1, "fixture files plus its stamp");
}
