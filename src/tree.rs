use anyhow::Result;
use binrw::BinRead;
use binrw::io::{Cursor, Read};
use flate2::read::ZlibDecoder;
use rangemap::RangeSet;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

#[derive(BinRead, Debug)]
#[br(big)]
pub struct Blob {
    #[br(assert(_size != 0))]
    _size: u32,
    #[br(count(_size))]
    bytes: Vec<u8>,
}

#[derive(BinRead, Debug)]
#[br(import { flags: u16 })]
pub enum EntryData {
    #[br(pre_assert(flags & 2 != 0))]
    Directory {
        count: u32,
        node_id: u32,
    },
    #[br(pre_assert(flags & 2 == 0))]
    File {
        locale: u32,
        data_offset: u32,
    },
}

#[derive(BinRead, Debug)]
#[br(big)]
pub struct Entry {
    name_offset: u32,
    flags: u16,
    #[br(args { flags })]
    data: EntryData,
    _last_modified: u64,
}

/// Attempts to parse a tree from the given byte array `bytes`. The node ID `node_id` and node
/// count `count` are used to extract the appropriate slice of tree entries from this byte array.
/// In addition, `node_ids` is used to keep track of node IDs that have already been visited.
///
/// While parsing each tree entry, the name offset is checked against the `name_offsets` HashSet to
/// ensure that the name offset is valid.
///
/// Yields 0 if any of the sanity checks failed. Otherwise returns the number of valid name offsets
/// that we have seen.
pub fn parse_tree(
    name_offsets: &HashSet<usize>,
    node_ids: &mut RangeSet<usize>,
    bytes: &[u8],
    node_id: usize,
    count: usize,
) -> usize {
    // Check that we have enough bytes for the node ID to make sense.
    if bytes.len() / 22 <= node_id {
        return 0;
    }

    // Check that we have enough bytes for the node count to make sense.
    if bytes.len() / 22 - node_id <= count {
        return 0;
    }

    // Check if we have seen any of the node IDs already.
    for id in node_id..node_id + count {
        if node_ids.contains(&id) {
            return 0;
        }
    }

    // Great! Let's track these nodes.
    node_ids.insert(node_id..node_id + count);

    // Parse the entries.
    let mut reader = Cursor::new(&bytes[node_id * 22..][..count * 22]);
    let mut result = 0;

    for _ in 0..count {
        // Read the current entry.
        let entry = match Entry::read(&mut reader) {
            Ok(entry) => entry,
            _ => return 0,
        };

        // Does the name offset correspond to any name in our set of names?
        if !name_offsets.contains(&(entry.name_offset as usize)) {
            return 0;
        }

        // Do the flags make sense?
        if entry.flags > 2 {
            return 0;
        }

        // Parse the directory.
        if let EntryData::Directory { node_id, count, .. } = entry.data {
            let count = parse_tree(name_offsets, node_ids, bytes, node_id as usize, count as usize);

            // OK, something failed while parsing the directory.
            if count == 0 {
                return 0;
            }

            result += count;
        }

        result += 1;
    }

    result
}

/// Parses the tree from the given byte array `bytes` using the node ID `node_id` and node count
/// `count` to extract a slice of the appropriate tree entries to collect all the data offsets.
///
/// Yields an ordered set of data offsets.
pub fn collect_data_offsets(
    bytes: &[u8],
    node_id: usize,
    count: usize,
) -> BTreeSet<usize> {
    let mut offsets = BTreeSet::new();

    // Check that we have enough bytes for the node ID to make sense.
    if bytes.len() / 22 <= node_id {
        return offsets;
    }

    // Check that we have enough bytes for the node count to make sense.
    if bytes.len() / 22 - node_id <= count {
        return offsets;
    }

    // Parse the entries.
    let mut reader = Cursor::new(&bytes[node_id * 22..][..count * 22]);

    for _ in 0..count {
        // Read the current entry.
        let entry = match Entry::read(&mut reader) {
            Ok(entry) => entry,
            _ => continue,
        };

        match entry.data {
            EntryData::Directory { node_id, count, .. } => {
                for offset in collect_data_offsets(bytes, node_id as usize, count as usize) {
                    offsets.insert(offset);
                }
            }
            EntryData::File { data_offset, .. } => {
                offsets.insert(data_offset as usize);
            }
        }
    }

    offsets
}

pub fn find_tree_offsets(
    names: &BTreeMap<usize, String>,
    bytes: &[u8],
) -> BTreeSet<usize> {
    let mut tree_offsets = BTreeSet::new();

    // Collect the name offsets.
    let name_offsets: HashSet<usize> = names
        .keys()
        .map(|offset| *offset)
        .collect();

    for offset in (0..bytes.len()).step_by(8).rev() {
        let mut node_ids = RangeSet::new();

        // Try parsing the current offset as a tree.
        let count = parse_tree(&name_offsets, &mut node_ids, &bytes[offset..], 0, 1);

        // Did this tree use all of our name offsets?
        if count >= name_offsets.len() {
            tree_offsets.insert(offset);
        }
    }

    tree_offsets
}

pub fn find_blob_offsets(
    tree_offset: usize,
    bytes: &[u8],
) -> BTreeSet<usize> {
    let mut blob_offsets = BTreeSet::new();

    let offsets = collect_data_offsets(&bytes[tree_offset..], 0, 1);
    let offsets: Vec<usize> = offsets.into_iter().collect();

    // Calculate the deltas between the ordered data offsets.
    let deltas: Vec<usize> = offsets
        .windows(2)
        .map(|pair| pair[1] - pair[0] - 4)
        .collect();

    if let Some(first) = deltas.first() {
        let first = *first;

        for (start, window) in bytes.windows(4).enumerate() {
            // Decode the 32-bit size field.
            let mut slice = [0u8; 4];
            slice.copy_from_slice(&window);
            let mut size = u32::from_be_bytes(slice) as usize;

            // Check if it matches with the first delta.
            if size != first {
                continue;
            }

            // Traverse the blobs.
            let mut offset = start;
            let mut found = true;

            for delta in &deltas[1..] {
                let delta = *delta;

                // Point to the next blob.
                offset = offset + size + 4;

                // Decode the 32-bit size field.
                let mut slice = [0u8; 4];
                slice.copy_from_slice(&bytes[offset..][..4]);
                size = u32::from_be_bytes(slice) as usize;

                // Check if it matches with the next delta in the chain.
                if size != delta {
                    found = false;
                    break;
                }
            }

            // Did we find a complete chain?
            if found {
                blob_offsets.insert(start);
            }
        }
    }

    blob_offsets
}

pub fn extract_tree<P: AsRef<Path>>(
    root: P,
    names: &BTreeMap<usize, String>,
    blobs: &[u8],
    bytes: &[u8],
    node_id: usize,
    count: usize,
) -> Result<()> {
    // Check that we have enough bytes for the node ID to make sense.
    if bytes.len() / 22 <= node_id {
        return Ok(());
    }

    // Check that we have enough bytes for the node count to make sense.
    if bytes.len() / 22 - node_id <= count {
        return Ok(());
    }

    // Parse the entries.
    let mut reader = Cursor::new(&bytes[node_id * 22..][..count * 22]);

    for _ in 0..count {
        // Read the current entry.
        let entry = match Entry::read(&mut reader) {
            Ok(entry) => entry,
            _ => continue,
        };

        // Clone the root path.
        let mut path = root.as_ref().to_path_buf();

        // Get the name of the entry.
        match names.get(&(entry.name_offset as usize)) {
            Some(name) => path.push(name),
            _ => continue,
        };

        match entry.data {
            EntryData::Directory { node_id, count, .. } => {
                std::fs::create_dir_all(&path)?;
                extract_tree(&path, names, blobs, bytes, node_id as usize, count as usize)?;
            }
            EntryData::File { data_offset, .. } => {
                let mut reader = Cursor::new(&blobs[data_offset as usize..]);

                // Parse the blob.
                let blob = match Blob::read(&mut reader) {
                    Ok(blob) => blob,
                    _ => continue,
                };

                let bytes = if entry.flags & 1 == 1 {
                    let mut bytes = vec![];
                    let mut z = ZlibDecoder::new(&blob.bytes[4..]);
                    z.read_to_end(&mut bytes)?;

                    bytes
                } else {
                    blob.bytes
                };

                println!("Extracting {}", path.display());
                std::fs::write(path, bytes)?;
            }
        }
    }

    Ok(())
}
