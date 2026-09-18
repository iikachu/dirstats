// SPDX-License-Identifier: Apache-2.0
// by dirstats contributors

//! Every variant must build the same tree as dirstats-scan.
#![cfg(target_os = "linux")]

use dirstats_archive_linux_walker::{LinuxWalker, Options, scan};
use dirstats_scan::{ScanOptions, SizeMetric, Tree};
use std::collections::BTreeMap;
use std::fs;

#[test]
fn every_variant_matches_dirstats_scan() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("a/b/c")).unwrap();
    fs::create_dir(root.join("empty")).unwrap();
    fs::create_dir(root.join("wide")).unwrap();
    // More entries than one stat chunk, so the chunks are shared out.
    for i in 0..1000 {
        fs::write(root.join("wide").join(format!("f{i}")), vec![7u8; i]).unwrap();
        if i % 100 == 0 {
            fs::create_dir(root.join("wide").join(format!("d{i}"))).unwrap();
            fs::write(root.join("wide").join(format!("d{i}/inner")), b"x").unwrap();
        }
    }
    fs::write(root.join("a/b/c/deep"), vec![1u8; 70_000]).unwrap();
    fs::write(root.join("ラウト.txt"), b"hello").unwrap();
    fs::hard_link(root.join("a/b/c/deep"), root.join("a/link")).unwrap();
    std::os::unix::fs::symlink(root.join("a"), root.join("to_a")).unwrap();

    let describe = |tree: &Tree| {
        tree.nodes()
            .map(|(id, n)| (tree.path(id), (n.kind, tree.size(id), n.file_count, n.dir_count, n.error, n.modified)))
            .collect::<BTreeMap<_, _>>()
    };
    for (name, walker) in LinuxWalker::variants() {
        // Which of two links counts as the duplicate depends on arrival
        // order, which differs between walkers; compare with both counted.
        let options = Options { threads: 3, count_hard_links_once: false, walker, ..Options::default() };
        let once = Options { count_hard_links_once: true, ..options.clone() };
        assert_eq!(scan(root, &once).unwrap().nodes().filter(|(_, n)| n.duplicate_link).count(), 1, "{name}");
        for metric in [SizeMetric::Apparent, SizeMetric::Allocated] {
            let ours = scan(root, &Options { size_metric: metric, ..options.clone() }).unwrap();
            let theirs = dirstats_scan::scan(
                root,
                &ScanOptions { threads: 3, count_hard_links_once: false, size_metric: metric, ..ScanOptions::default() },
            )
            .unwrap();
            assert_eq!(describe(&ours), describe(&theirs), "{name}, {metric:?}");
        }
    }
}
