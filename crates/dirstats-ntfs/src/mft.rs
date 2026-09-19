// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors
//
// Port of the record parsing in WinDirStat's `FinderNtfs.cpp`
// (GPL-2.0-or-later, by WinDirStat Team, https://windirstat.net).

//! Master file table records: parsing and turning them into a [`Tree`].
//!
//! Nothing here touches the operating system, so it builds and is tested
//! on every platform.

use dirstats_scan::{Kind, Node, NodeId, ScanOptions, Tree, TreeBuilder};
use foldhash::{HashMap, HashSet};
use std::ffi::OsString;
use std::time::{Duration, SystemTime};

/// Record number of a volume's root directory.
pub const ROOT_RECORD: u64 = 5;

const ATTRIBUTE_STANDARD_INFORMATION: u32 = 0x10;
const ATTRIBUTE_FILE_NAME: u32 = 0x30;
const ATTRIBUTE_DATA: u32 = 0x80;
const ATTRIBUTE_REPARSE_POINT: u32 = 0xC0;
const ATTRIBUTE_END: u32 = 0xFFFF_FFFF;

const RECORD_IN_USE: u16 = 0x0001;
const RECORD_IS_DIRECTORY: u16 = 0x0002;
const ATTRIBUTE_COMPRESSED: u16 = 0x0001;
const ATTRIBUTE_SPARSE: u16 = 0x8000;
/// A `FILE_NAME` holding only the 8.3 alias of a name listed elsewhere.
const NAME_DOS_ONLY: u8 = 0x02;

const REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
const REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;

/// Update sequence fixups protect every 512 bytes, whatever the sector size.
const FIXUP_STRIDE: usize = 512;

/// What is known about one file, gathered from its base record and any
/// extension records.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Record {
    /// Set by the base record's header flags.
    pub is_directory: bool,
    /// Length of the unnamed data stream in bytes.
    pub logical_size: Option<u64>,
    /// Allocation of the unnamed data stream in bytes: the compressed size
    /// for a compressed or sparse stream, and the length rounded up to 8 for
    /// a resident one. `None` when the table reports zero.
    pub physical_size: Option<u64>,
    /// Allocation of the `WofCompressedData` stream, which holds the real
    /// data of a file compressed by the Windows overlay filter.
    pub wof_physical_size: Option<u64>,
    /// Last modification as a `FILETIME`.
    pub modified: Option<u64>,
    /// Tag of the reparse point, if the file is one.
    pub reparse_tag: Option<u32>,
}

impl Record {
    /// Fold in what another record (or another thread) found for the same
    /// file; values present in `other` win.
    fn merge(&mut self, other: Record) {
        self.is_directory |= other.is_directory;
        self.logical_size = other.logical_size.or(self.logical_size);
        self.physical_size = other.physical_size.or(self.physical_size);
        self.wof_physical_size = other.wof_physical_size.or(self.wof_physical_size);
        self.modified = other.modified.or(self.modified);
        self.reparse_tag = other.reparse_tag.or(self.reparse_tag);
    }
}

/// One name of a file. A file has several when it is hard linked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    /// Record number of the containing directory, sequence number dropped.
    pub parent: u64,
    /// Base record number of the file.
    pub record: u64,
    /// UTF-16 code units, as stored. 8.3-only aliases and `.`/`..` are
    /// never recorded.
    pub name: Vec<u16>,
}

/// Everything read from the table so far. Each reader thread fills its own
/// and they are merged at the end, as one file's records can be far apart.
#[derive(Debug, Default)]
pub struct Table {
    /// Files by base record number.
    pub records: HashMap<u64, Record>,
    /// Every name found, in no particular order.
    pub names: Vec<Name>,
}

impl Table {
    /// Take in everything another table read.
    pub fn merge(&mut self, other: Table) {
        for (number, record) in other.records {
            self.records.entry(number).or_default().merge(record);
        }
        self.names.extend(other.names);
    }

    /// Parse a run of whole records. `first_record` is the number of the
    /// record at the start of `buffer`, which is patched in place by the
    /// fixups. Records that are unused, lack the `FILE` signature or fail
    /// their fixups are skipped; a malformed attribute ends its record,
    /// keeping what was read before it. A trailing partial record is ignored.
    pub fn parse_records(&mut self, buffer: &mut [u8], record_size: usize, first_record: u64) {
        if record_size == 0 {
            return;
        }
        for (i, record) in buffer.chunks_exact_mut(record_size).enumerate() {
            let _ = self.parse_record(record, first_record + i as u64);
        }
    }

    /// `None` when the record is unused or damaged.
    fn parse_record(&mut self, record: &mut [u8], number: u64) -> Option<()> {
        if record.get(..4)? != b"FILE" {
            return None;
        }
        apply_fixups(record)?;
        let flags = u16_at(record, 22)?;
        if flags & RECORD_IN_USE == 0 {
            return None;
        }
        let base = u64_at(record, 32)? & 0xFFFF_FFFF_FFFF;
        let is_base = base == 0;
        let base = if is_base { number } else { base };
        let entry = self.records.entry(base).or_default();
        entry.is_directory |= is_base && flags & RECORD_IS_DIRECTORY != 0;

        let mut offset = usize::from(u16_at(record, 20)?);
        loop {
            let attribute = record.get(offset..)?;
            let type_code = u32_at(attribute, 0)?;
            if type_code == ATTRIBUTE_END {
                return Some(());
            }
            let length = u32_at(attribute, 4)? as usize;
            if length == 0 {
                return Some(());
            }
            let attribute = attribute.get(..length)?;
            let non_resident = *attribute.get(8)? & 1 != 0;
            match type_code {
                ATTRIBUTE_STANDARD_INFORMATION if !non_resident => {
                    entry.modified = Some(u64_at(resident_value(attribute)?, 8)?);
                }
                ATTRIBUTE_FILE_NAME if !non_resident => {
                    let value = resident_value(attribute)?;
                    let units = usize::from(*value.get(64)?);
                    let name: Vec<u16> =
                        value.get(66..66 + units * 2)?.as_chunks::<2>().0.iter().map(|&pair| u16::from_le_bytes(pair)).collect();
                    let dot = u16::from(b'.');
                    if *value.get(65)? != NAME_DOS_ONLY && name != [dot] && name != [dot, dot] {
                        let parent = u64_at(value, 0)? & 0xFFFF_FFFF_FFFF;
                        self.names.push(Name { parent, record: base, name });
                    }
                }
                ATTRIBUTE_DATA => parse_data(entry, attribute, non_resident)?,
                ATTRIBUTE_REPARSE_POINT if !non_resident => {
                    entry.reparse_tag = Some(u32_at(resident_value(attribute)?, 0)?);
                }
                _ => {}
            }
            offset += length;
        }
    }

    /// Build the tree under the volume's root directory, which is named
    /// `root_name`. `None` when the table holds no root.
    ///
    /// Only files reachable from the root by name are placed. A directory
    /// with several names is placed once; a file's further names become
    /// [`Node::duplicate_link`] entries when
    /// [`ScanOptions::count_hard_links_once`] is set, and count in full
    /// otherwise. Which name of a hard-linked file comes first is not
    /// specified.
    #[must_use]
    pub fn into_tree(mut self, root_name: OsString, options: &ScanOptions) -> Option<Tree> {
        let root = *self.records.get(&ROOT_RECORD)?;
        self.names.sort_unstable_by_key(|name| name.parent);
        let mut children: HashMap<u64, std::ops::Range<usize>> = HashMap::default();
        for (i, name) in self.names.iter().enumerate() {
            children.entry(name.parent).or_insert(i..i).end = i + 1;
        }

        let mut builder = TreeBuilder::new();
        let root_id = builder.push(node(root_name.into_boxed_os_str(), None, &root));
        // Every record placed so far: a second name for a file is a hard
        // link, and a directory is never entered twice.
        let mut placed: HashSet<u64> = HashSet::default();
        placed.insert(ROOT_RECORD);
        let mut queue = std::collections::VecDeque::from([(ROOT_RECORD, root_id)]);
        while let Some((directory, parent)) = queue.pop_front() {
            let Some(range) = children.get(&directory) else { continue };
            for name in &self.names[range.clone()] {
                let Some(record) = self.records.get(&name.record) else { continue };
                let first = placed.insert(name.record);
                if record.is_directory && !first {
                    continue;
                }
                let mut node = node(os_string(&name.name).into_boxed_os_str(), Some(parent), record);
                node.duplicate_link = !first && options.count_hard_links_once;
                let id = builder.push(node);
                if node_kind(record) == Kind::Directory {
                    queue.push_back((name.record, id));
                }
            }
        }
        Some(builder.finish(options.size_metric))
    }
}

/// Record the sizes of a `DATA` attribute: the unnamed stream's length and
/// allocation, or the allocation of a `WofCompressedData` stream. Other
/// named streams are ignored.
fn parse_data(entry: &mut Record, attribute: &[u8], non_resident: bool) -> Option<()> {
    let name_units = usize::from(*attribute.get(9)?);
    // Later pieces of a stream split over several records repeat its sizes as zero.
    let first_piece = !non_resident || u64_at(attribute, 16)? == 0;
    let resident_sizes = |attribute| Some(u64::from(u32_at(attribute, 16)?));

    if name_units > 0 {
        let name_offset = usize::from(u16_at(attribute, 10)?);
        let name = attribute.get(name_offset..name_offset + name_units * 2)?;
        let is_wof = name.as_chunks::<2>().0.iter().map(|&pair| u16::from_le_bytes(pair)).eq("WofCompressedData".encode_utf16());
        if is_wof && first_piece {
            entry.wof_physical_size =
                Some(if non_resident { u64_at(attribute, 40)? } else { resident_sizes(attribute)?.next_multiple_of(8) });
        }
        return Some(());
    }

    if !non_resident {
        let length = resident_sizes(attribute)?;
        entry.logical_size = Some(length);
        entry.physical_size = Some(length.next_multiple_of(8));
    } else if first_piece {
        entry.logical_size = Some(u64_at(attribute, 48)?);
        let flags = u16_at(attribute, 12)?;
        let physical = if flags & (ATTRIBUTE_COMPRESSED | ATTRIBUTE_SPARSE) != 0 { u64_at(attribute, 64)? } else { u64_at(attribute, 40)? };
        if physical > 0 {
            entry.physical_size = Some(physical);
        }
    }
    Some(())
}

/// Replace the check word at the end of every 512 bytes with the original
/// data. `None` when a check word does not match: a torn write.
fn apply_fixups(record: &mut [u8]) -> Option<()> {
    let offset = usize::from(u16_at(record, 4)?);
    let count = usize::from(u16_at(record, 6)?);
    if count == 0 {
        return Some(());
    }
    let check = u16_at(record, offset)?;
    for i in 1..count {
        let original = u16_at(record, offset + i * 2)?;
        let end = i * FIXUP_STRIDE;
        if u16_at(record, end.checked_sub(2)?)? != check {
            return None;
        }
        record[end - 2..end].copy_from_slice(&original.to_le_bytes());
    }
    Some(())
}

/// The value of a resident attribute; `None` if it runs past the attribute.
fn resident_value(attribute: &[u8]) -> Option<&[u8]> {
    let length = u32_at(attribute, 16)? as usize;
    let offset = usize::from(u16_at(attribute, 20)?);
    attribute.get(offset..offset.checked_add(length)?)
}

/// Symlinks and mount points (junctions) are [`Kind::Symlink`] and never
/// entered.
fn node_kind(record: &Record) -> Kind {
    match record.reparse_tag {
        // Their targets are listed where they really live, or on another volume.
        Some(REPARSE_TAG_SYMLINK | REPARSE_TAG_MOUNT_POINT) => Kind::Symlink,
        _ if record.is_directory => Kind::Directory,
        _ => Kind::File,
    }
}

/// A tree node for `record`. Directories get no size of their own, only
/// their contents'; an overlay-compressed file's allocation is that of its
/// compressed stream.
fn node(name: Box<std::ffi::OsStr>, parent: Option<NodeId>, record: &Record) -> Node {
    let kind = node_kind(record);
    let is_directory = kind == Kind::Directory;
    Node {
        name,
        parent,
        kind,
        apparent_size: if is_directory { 0 } else { record.logical_size.unwrap_or(0) },
        allocated_size: if is_directory { 0 } else { record.wof_physical_size.or(record.physical_size).unwrap_or(0) },
        file_count: u64::from(!is_directory),
        dir_count: 0,
        modified: record.modified.and_then(system_time),
        duplicate_link: false,
        error: false,
    }
}

/// `FILETIME` counts 100 ns ticks from 1601. `None` before 1970.
fn system_time(filetime: u64) -> Option<SystemTime> {
    const TICKS_TO_UNIX_EPOCH: u64 = 116_444_736_000_000_000;
    let ticks = filetime.checked_sub(TICKS_TO_UNIX_EPOCH)?;
    SystemTime::UNIX_EPOCH.checked_add(Duration::new(ticks / 10_000_000, (ticks % 10_000_000) as u32 * 100))
}

#[cfg(windows)]
fn os_string(units: &[u16]) -> OsString {
    std::os::windows::ffi::OsStringExt::from_wide(units)
}

/// Unpaired surrogates become U+FFFD, as there is no lossless form here.
#[cfg(not(windows))]
fn os_string(units: &[u16]) -> OsString {
    String::from_utf16_lossy(units).into()
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?))
}

fn u64_at(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORD_SIZE: usize = 1024;
    const CHECK: u16 = 0xBEEF;

    fn attribute(type_code: u32, non_resident: bool, flags: u16, stream: &str, body: &[u8]) -> Vec<u8> {
        let stream: Vec<u8> = stream.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let name_offset = if non_resident { 72 } else { 24 };
        let mut a = vec![0u8; name_offset];
        a[0..4].copy_from_slice(&type_code.to_le_bytes());
        a[8] = u8::from(non_resident);
        a[9] = (stream.len() / 2) as u8;
        a[10..12].copy_from_slice(&(name_offset as u16).to_le_bytes());
        a[12..14].copy_from_slice(&flags.to_le_bytes());
        a.extend(&stream);
        if non_resident {
            a[16..72].copy_from_slice(body);
        } else {
            let value_offset = a.len().next_multiple_of(8);
            a.resize(value_offset, 0);
            a[16..20].copy_from_slice(&(body.len() as u32).to_le_bytes());
            a[20..22].copy_from_slice(&(value_offset as u16).to_le_bytes());
            a.extend(body);
        }
        let length = a.len().next_multiple_of(8);
        a.resize(length, 0);
        a[4..8].copy_from_slice(&(length as u32).to_le_bytes());
        a
    }

    fn file_name(parent: u64, name: &str, flags: u8) -> Vec<u8> {
        let mut v = vec![0u8; 66];
        v[0..8].copy_from_slice(&(parent | 7 << 48).to_le_bytes());
        v[64] = name.encode_utf16().count() as u8;
        v[65] = flags;
        v.extend(name.encode_utf16().flat_map(u16::to_le_bytes));
        attribute(ATTRIBUTE_FILE_NAME, false, 0, "", &v)
    }

    fn standard_information(modified: u64) -> Vec<u8> {
        let mut v = vec![0u8; 48];
        v[8..16].copy_from_slice(&modified.to_le_bytes());
        attribute(ATTRIBUTE_STANDARD_INFORMATION, false, 0, "", &v)
    }

    /// Non-resident stream body: bytes 16..72 of the attribute.
    fn non_resident(lowest_vcn: u64, allocated: u64, size: u64, compressed: u64) -> Vec<u8> {
        let mut v = vec![0u8; 56];
        v[0..8].copy_from_slice(&lowest_vcn.to_le_bytes());
        v[24..32].copy_from_slice(&allocated.to_le_bytes());
        v[32..40].copy_from_slice(&size.to_le_bytes());
        v[48..56].copy_from_slice(&compressed.to_le_bytes());
        v
    }

    fn record(flags: u16, base: u64, attributes: &[Vec<u8>]) -> Vec<u8> {
        let mut r = vec![0u8; RECORD_SIZE];
        r[0..4].copy_from_slice(b"FILE");
        r[4..6].copy_from_slice(&48u16.to_le_bytes());
        r[6..8].copy_from_slice(&3u16.to_le_bytes());
        r[20..22].copy_from_slice(&56u16.to_le_bytes());
        r[22..24].copy_from_slice(&flags.to_le_bytes());
        r[32..40].copy_from_slice(&base.to_le_bytes());
        let mut offset = 56;
        for a in attributes {
            r[offset..offset + a.len()].copy_from_slice(a);
            offset += a.len();
        }
        r[offset..offset + 4].copy_from_slice(&ATTRIBUTE_END.to_le_bytes());
        // Move the true sector-end words into the fixup array.
        r[48..50].copy_from_slice(&CHECK.to_le_bytes());
        for i in 1..3 {
            let end = i * FIXUP_STRIDE;
            let original = [r[end - 2], r[end - 1]];
            r[48 + i * 2..50 + i * 2].copy_from_slice(&original);
            r[end - 2..end].copy_from_slice(&CHECK.to_le_bytes());
        }
        r
    }

    fn table(records: &[(u64, Vec<u8>)]) -> Table {
        let mut table = Table::default();
        for (number, bytes) in records {
            table.parse_records(&mut bytes.clone(), RECORD_SIZE, *number);
        }
        table
    }

    fn find<'a>(tree: &'a Tree, parent: NodeId, name: &str) -> (NodeId, &'a Node) {
        let id = *tree.children(parent).iter().find(|&&id| &*tree.node(id).name == name).expect(name);
        (id, tree.node(id))
    }

    #[test]
    fn builds_a_tree_with_sizes_links_and_reparse_points() {
        let modified = 116_444_736_000_000_000 + 10_000_000;
        let in_use = RECORD_IN_USE;
        let dir = RECORD_IN_USE | RECORD_IS_DIRECTORY;
        let table = table(&[
            (5, record(dir, 0, &[file_name(5, ".", 3)])),
            (40, record(dir, 0, &[file_name(5, "docs", 1)])),
            // Resident data, with an 8.3 alias that must not show up.
            (
                41,
                record(
                    in_use,
                    0,
                    &[
                        standard_information(modified),
                        file_name(40, "LONGNA~1.TXT", NAME_DOS_ONLY),
                        file_name(40, "long name.txt", 1),
                        attribute(ATTRIBUTE_DATA, false, 0, "", &[1; 13]),
                    ],
                ),
            ),
            // Hard linked, its data attribute held by an extension record.
            (42, record(in_use, 0, &[file_name(5, "big.bin", 3), file_name(40, "link.bin", 3)])),
            (
                43,
                record(
                    in_use,
                    42,
                    &[
                        attribute(ATTRIBUTE_DATA, true, 0, "", &non_resident(0, 8192, 5000, 0)),
                        attribute(ATTRIBUTE_DATA, true, 0, "", &non_resident(2, 0, 0, 0)),
                    ],
                ),
            ),
            // Sparse: the compressed size is what is allocated.
            (
                44,
                record(
                    in_use,
                    0,
                    &[
                        file_name(5, "sparse", 1),
                        attribute(ATTRIBUTE_DATA, true, ATTRIBUTE_SPARSE, "", &non_resident(0, 1 << 20, 1 << 20, 4096)),
                    ],
                ),
            ),
            // Overlay-compressed: the named stream holds the real data.
            (
                45,
                record(
                    in_use,
                    0,
                    &[
                        file_name(5, "wof.exe", 1),
                        attribute(ATTRIBUTE_DATA, true, ATTRIBUTE_SPARSE, "", &non_resident(0, 65536, 60000, 0)),
                        attribute(ATTRIBUTE_DATA, true, 0, "WofCompressedData", &non_resident(0, 16384, 15000, 0)),
                        attribute(ATTRIBUTE_DATA, false, 0, "Zone.Identifier", &[0; 26]),
                    ],
                ),
            ),
            (
                46,
                record(
                    dir,
                    0,
                    &[
                        file_name(5, "junction", 1),
                        attribute(ATTRIBUTE_REPARSE_POINT, false, 0, "", &REPARSE_TAG_MOUNT_POINT.to_le_bytes()),
                    ],
                ),
            ),
            // Deleted: not in use.
            (47, record(0, 0, &[file_name(5, "deleted", 1)])),
        ]);

        let tree = table.into_tree("C:\\".into(), &ScanOptions::default()).unwrap();
        let root = tree.root();
        assert_eq!(tree.len(), 8);

        let (docs, _) = find(&tree, root, "docs");
        let (_, text) = find(&tree, docs, "long name.txt");
        assert_eq!((text.apparent_size, text.allocated_size), (13, 16));
        assert_eq!(text.modified, Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1)));
        assert_eq!(tree.children(docs).len(), 2);

        let (_, big) = find(&tree, root, "big.bin");
        let (_, link) = find(&tree, docs, "link.bin");
        assert_eq!((big.apparent_size, big.allocated_size), (5000, 8192));
        assert_ne!(big.duplicate_link, link.duplicate_link);

        assert_eq!(find(&tree, root, "sparse").1.allocated_size, 4096);
        let (_, wof) = find(&tree, root, "wof.exe");
        assert_eq!((wof.apparent_size, wof.allocated_size), (60000, 16384));
        assert_eq!(find(&tree, root, "junction").1.kind, Kind::Symlink);

        let root = tree.node(root);
        assert_eq!(root.allocated_size, 16 + 8192 + 4096 + 16384);
        assert_eq!((root.file_count, root.dir_count), (6, 1));
    }

    #[test]
    fn skips_torn_and_truncated_records() {
        let mut torn = record(RECORD_IN_USE, 0, &[file_name(5, "torn", 1)]);
        torn[510] ^= 0xFF;
        let mut endless = record(RECORD_IN_USE, 0, &[file_name(5, "endless", 1)]);
        // An attribute claiming to run past the record.
        endless[60..64].copy_from_slice(&5000u32.to_le_bytes());
        let table = table(&[(50, torn), (51, endless), (52, vec![0; RECORD_SIZE])]);
        assert!(table.names.is_empty());
        assert!(table.into_tree("C:\\".into(), &ScanOptions::default()).is_none());
    }
}
