// SPDX-License-Identifier: Apache-2.0
// by dirstats contributors

//! Times every Linux walker variant against dua-core's walker on one root
//! and prints a Markdown table. Built in release by the `linux-walker-bench`
//! CI job, which runs it on ext4, XFS and NFS.
//!
//! ```text
//! cargo build --release -p dirstats-scan --example walkbench
//! sudo target/release/examples/walkbench --root /usr --label "ext4 /usr" --reps 5 --cold
//! ```
//!
//! Variants run round-robin, with the order rotated each round, so drift in
//! the machine's speed is spread evenly. With `--cold` (Linux, root) the
//! page, dentry and inode caches are dropped before every scan. Every
//! scan's file count and total size are checked against dua-core's.

use dirstats_scan::{LinuxWalker, ScanOptions, SizeMetric, linux_io_uring_available, scan};
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct Args {
    root: PathBuf,
    label: String,
    reps: usize,
    cold: bool,
    /// Substrings of variant names to run; all when empty.
    only: Vec<String>,
    /// Substrings of variant names to leave out.
    skip: Vec<String>,
}

fn args() -> Args {
    let mut args = Args { root: PathBuf::from("/usr"), label: String::new(), reps: 5, cold: false, only: Vec::new(), skip: Vec::new() };
    let mut raw = std::env::args().skip(1);
    while let Some(arg) = raw.next() {
        match arg.as_str() {
            "--root" => args.root = raw.next().expect("--root PATH").into(),
            "--label" => args.label = raw.next().expect("--label TEXT"),
            "--reps" => args.reps = raw.next().expect("--reps N").parse().expect("--reps N"),
            "--cold" => args.cold = true,
            "--only" => args.only.push(raw.next().expect("--only NAME")),
            "--skip" => args.skip.push(raw.next().expect("--skip NAME")),
            other => panic!("unknown argument {other}"),
        }
    }
    if args.label.is_empty() {
        args.label = args.root.display().to_string();
    }
    args
}

fn drop_caches() {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn sync();
        }
        // SAFETY: sync takes no arguments and cannot fail.
        unsafe { sync() };
    }
    std::fs::write("/proc/sys/vm/drop_caches", "3").expect("--cold needs root on Linux");
}

fn median(times: &mut [Duration]) -> Duration {
    times.sort();
    times[times.len() / 2]
}

fn main() {
    let args = args();
    let variants: Vec<(&str, LinuxWalker)> = LinuxWalker::variants()
        .into_iter()
        .enumerate()
        .filter(|(i, (name, _))| {
            let wanted = args.only.is_empty() || args.only.iter().any(|o| name.contains(o.as_str()));
            *i == 0 || (wanted && !args.skip.iter().any(|s| name.contains(s.as_str())))
        })
        .map(|(_, v)| v)
        .collect();
    let mut times = vec![Vec::new(); variants.len()];
    let mut shapes = vec![None; variants.len()];
    for round in 0..args.reps {
        for step in 0..variants.len() {
            let index = (round + step) % variants.len();
            let (name, linux) = &variants[index];
            let options = ScanOptions { linux: linux.clone(), size_metric: SizeMetric::Allocated, ..ScanOptions::default() };
            if args.cold {
                drop_caches();
            }
            let start = Instant::now();
            let tree = scan(&args.root, &options).expect("scan");
            let elapsed = start.elapsed();
            let root = tree.root();
            let shape = (tree.node(root).file_count, tree.node(root).dir_count, tree.size(root));
            eprintln!("{} round {round}: {name}: {elapsed:.2?} {shape:?}", args.label);
            times[index].push(elapsed);
            shapes[index].get_or_insert(shape);
        }
    }

    let cache = if args.cold { "cold" } else { "warm" };
    let threads = ScanOptions::default().threads;
    println!("### {} ({cache} cache, {} runs each, {threads} threads)\n", args.label, args.reps);
    let (files, dirs, bytes) = shapes[0].unwrap();
    println!("dua-core saw {files} files, {dirs} directories, {:.2} GiB allocated.", bytes as f64 / f64::from(1 << 30));
    println!("io_uring available: {}.\n", linux_io_uring_available());
    println!("| Walker | Median | Best | vs dua-core | Same result |");
    println!("|---|---:|---:|---:|---|");
    let baseline = median(&mut times[0].clone());
    for (index, (name, _)) in variants.iter().enumerate() {
        let best = *times[index].iter().min().unwrap();
        let mid = median(&mut times[index]);
        let ratio = baseline.as_secs_f64() / mid.as_secs_f64();
        let same = if shapes[index] == shapes[0] { "yes".to_string() } else { format!("**no**: {:?}", shapes[index].unwrap()) };
        println!("| {name} | {:.3} s | {:.3} s | {ratio:.2}× | {same} |", mid.as_secs_f64(), best.as_secs_f64());
    }
    println!();
}
