use std::collections::{BTreeMap, BTreeSet};

use crate::executable::ExecutableMapping;

pub fn find_blobs_push(
    bytes: &[u8],
    mapping: &ExecutableMapping,
    tree_offset: usize,
    name_offset: usize,
) -> BTreeMap<usize, usize> {
    let mut known_offsets = BTreeSet::new();

    for (offset, window) in bytes.windows(5).enumerate() {
        // Look for the push instruction.
        if window[0] != 0x68 {
            continue;
        }

        // Decode the relative offset.
        let mut slice = [0u8; 4];
        slice.copy_from_slice(&window[1..]);
        let value = u32::from_ne_bytes(slice) as usize;

        // Look up the file offset.
        let value = match mapping.rva_to_file_offset(value) {
            Some(value) => value,
            _ => continue,
        };

        // Check if we found a push with the right offset.
        if value == tree_offset || value == name_offset {
            known_offsets.insert(offset);
        }
    }

    // Now that we have a set of known offsets, we can try and find the push instruction referencing
    // the blob offset.
    let known_offsets: Vec<usize> = known_offsets.into_iter().collect();
    let mut scores: BTreeMap<usize, usize> = BTreeMap::new();

    if known_offsets.is_empty() {
        return scores;
    }

    for (offset, window) in bytes.windows(5).enumerate() {
        // Look for the push instruction.
        if window[0] != 0x68 {
            continue;
        }

        // Decode the relative offset.
        let mut slice = [0u8; 4];
        slice.copy_from_slice(&window[1..]);
        let value = u32::from_ne_bytes(slice) as usize;

        // Look up the file offset.
        let value = match mapping.rva_to_file_offset(value) {
            Some(value) => value,
            _ => continue,
        };

        // Find the closest known offset to this instruction.
        let closest = match known_offsets.binary_search(&offset) {
            Ok(index) => continue,
            Err(index) => if index >= known_offsets.len() {
                known_offsets[index - 1]
            } else if index == 0 {
                known_offsets[0]
            } else {
                let lhs = known_offsets[index - 1];
                let rhs = known_offsets[index];

                if lhs.abs_diff(offset) < rhs.abs_diff(offset) {
                    lhs
                } else {
                    rhs
                }
            }
        };

        // Track the value and distance score.
        scores.insert(offset.abs_diff(closest), value);
    }

    scores
}

pub fn find_blobs_lea(
    bytes: &[u8],
    mapping: &ExecutableMapping,
    tree_offset: usize,
    name_offset: usize,
    is_win: bool,
) -> BTreeMap<usize, usize> {
    let mut known_offsets = BTreeSet::new();

    let (tree_reg, name_reg, blob_reg) = if is_win {
        // RDX, R8, R9
        (0x15, 0x05, 0x0d)
    } else {
        // RSI, RDX, RCX
        (0x35, 0x15, 0x0d)
    };

    for (offset, window) in bytes.windows(6).enumerate() {
        // Look for the lea instruction.
        if window[0] != 0x8d {
            continue;
        }

        // Decode the relative offset.
        let mut slice = [0u8; 4];
        slice.copy_from_slice(&window[2..]);
        let value = u32::from_ne_bytes(slice) as usize;

        // Calculate the absolute address.
        let value = offset + value + 6;

        // Look up the file offset.
        let value = match mapping.rva_to_file_offset(value) {
            Some(value) => value,
            _ => continue,
        };

        // Check if we found a lea with the right tree offset.
        if window[1] == tree_reg && value == tree_offset {
            known_offsets.insert(offset);
        }

        // Check if we found a lea with the right name offset.
        if window[1] == name_reg && value == name_offset {
            known_offsets.insert(offset);
        }
    }

    // Now that we have a set of known offsets, we can try and find the lea instruction referencing
    // the blob offset.
    let known_offsets: Vec<usize> = known_offsets.into_iter().collect();
    let mut scores: BTreeMap<usize, usize> = BTreeMap::new();

    if known_offsets.is_empty() {
        return scores;
    }

    for (offset, window) in bytes.windows(6).enumerate() {
        // Look for lea with the right destination register.
        if window[0] != 0x8d || window[1] != blob_reg {
            continue;
        }

        // Decode the relative offset.
        let mut slice = [0u8; 4];
        slice.copy_from_slice(&window[2..]);
        let value = u32::from_ne_bytes(slice) as usize;

        // Calculate the absolute address.
        let value = offset + value + 6;

        // Look up the file offset.
        let value = match mapping.rva_to_file_offset(value) {
            Some(value) => value,
            _ => continue,
        };

        // Find the closest known offset to this instruction.
        let closest = match known_offsets.binary_search(&offset) {
            Ok(index) => known_offsets[index],
            Err(index) => if index >= known_offsets.len() {
                known_offsets[index - 1]
            } else if index == 0 {
                known_offsets[0]
            } else {
                let lhs = known_offsets[index - 1];
                let rhs = known_offsets[index];

                if lhs.abs_diff(offset) < rhs.abs_diff(offset) {
                    lhs
                } else {
                    rhs
                }
            }
        };

        // Track the value and distance score.
        scores.insert(offset.abs_diff(closest), value);
    }

    scores
}
