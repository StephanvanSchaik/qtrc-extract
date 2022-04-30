use rangemap::RangeSet;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

/// Hashes the string following the algorithm implemented by QHash in Qt.
pub fn hash_str(s: &str) -> u32 {
    let mut h = 0;

    for c in s.chars() {
        h = (h << 4) + (c as u32);

        let g = h & 0xf0000000;

        if g != 0 {
            h ^= g >> 23;
        }

        h &= !g;
    }

    h
}

/// Scans the given byte array for name entries using the fact that commonly filenames will
/// completely fall within the ASCII range as a heuristic. Since the name is stored as UTF-16 BE,
/// this means that the code points are following the pattern 00 XX where XX will be within the
/// ASCII range. For each collected string, we then try to decode the 16-bit size field as well as
/// the 32-bit hash to confirm that we are indeed looking at a name entry, of which we record the
/// found offsets.
///
/// The `delta` argument is used to decide whether to look at odd or even offsets, since this
/// function looks at consecutive byte pairs.
pub fn scan_ascii_names(
    offsets: &mut BTreeSet<usize>, 
    bytes: &[u8],
    mut delta: usize,
) {
    // Each name entry starts with a 16-bit size and a 32-bit hash. We need at least eight bytes to
    // make this worthwhile.
    if bytes.len() < 6 {
        return;
    }

    delta += 6;

    let mut s = String::new();
    let mut start = 0;

    // Look at the bytes in chunks of two bytes.
    for (offset, pair) in bytes[6..].chunks_exact(2).enumerate() {
        // Calculate the actual byte offset.
        let offset = 2 * offset + delta;

        // We assume the pair of bytes is a UTF-16 codepoint that falls within the ASCII range. If
        // it is then we append to the string that we found so far.
        let c = pair[1] as char;

        if pair[0] == 0 && c.is_ascii_graphic() {
            s.push(c);
            continue;
        }

        // We found a code point that is not a UTF-16 codepoint that falls within the ASCII range.
        // Did we get a string to process?
        if s.is_empty() {
            start = offset + 2;
            continue;
        }

        // Decode the 16-bit size field.
        let mut slice = [0u8; 2];
        slice.copy_from_slice(&bytes[start - 6..][..2]);
        let size = u16::from_be_bytes(slice) as usize;

        // Check if the size is non-zero, and that we have collected enough codepoints.
        if size == 0 || s.len() < size {
            s = String::new();
            start = offset + 2;
            continue;
        }

        // Truncate the string to the size indicated by the size field..
        let (sub, _) = s.split_at(size);

        // Decode the 32-bit hash field.
        let mut slice = [0u8; 4];
        slice.copy_from_slice(&bytes[start - 4..][..4]);
        let hash = u32::from_be_bytes(slice);

        // Hash the string and check if the hashes match.
        if hash_str(sub) == hash {
            offsets.insert(start - 6);
        }

        // Reset the state.
        s = String::new();
        start = offset + 2;
    }
}

/// Parses name entries starting at the offset in the given byte array. Checks that the 16-bit size
/// field is non-zero, the 32-bit hash is valid and the string decodes into an actual UTF-16 BE
/// string for each name entry. Yields the parsed range as well a map of the relative offset to the
/// actual name.
pub fn parse_names(
    bytes: &[u8],
    mut offset: usize,
) -> (Range<usize>, BTreeMap<usize, String>) {
    let start = offset;
    let mut end = offset;

    let mut names = BTreeMap::new();
    let mut name = vec![];

    while offset < bytes.len() {
        // Decode the 16-bit size.
        let mut slice = [0u8; 2];
        slice.copy_from_slice(&bytes[offset..][..2]);
        let size = u16::from_be_bytes(slice) as usize;
        offset += 2;

        if size == 0 {
            break;
        }

        // Decode the 32-bit hash.
        let mut slice = [0u8; 4];
        slice.copy_from_slice(&bytes[offset..][..4]);
        let hash = u32::from_be_bytes(slice);
        offset += 4;

        // Decode the UTF-16 BE string.
        name.clear();

        for _ in 0..size {
            let mut slice = [0u8; 2];
            slice.copy_from_slice(&bytes[offset..][..2]);
            name.push(u16::from_be_bytes(slice));
            offset += 2;
        }

        let name = match String::from_utf16(&name) {
            Ok(name) => name,
            _ => break,
        };

        if hash_str(&name) != hash {
            break;
        }

        names.insert(end - start, name);
        end = offset;
    }

    (start..end, names)
}

/// Scans the given byte array for name entries and parses them. Yields a range map that maps
/// parsed byte ranges to a map of relative offsets to strings.
pub fn scan_names(
    bytes: &[u8],
) ->  BTreeMap<usize, (Range<usize>, BTreeMap<usize, String>)> {
    // Scan the byte array for name entries.
    let mut offsets = BTreeSet::new();
    scan_ascii_names(&mut offsets, &bytes[0..], 0);
    scan_ascii_names(&mut offsets, &bytes[1..], 1);

    // Keep track of the ranges we already parsed.
    let mut ranges = RangeSet::new();
    let mut sections = BTreeMap::new();

    for offset in &offsets {
        // We already parsed this offset, skip it.
        if ranges.contains(offset) {
            continue;
        }

        let offset = *offset;

        // Parse the name entries starting at the current offset.
        let (range, names) = parse_names(&bytes, offset);

        // We didn't find anything?
        if range.is_empty() {
            continue;
        }

        ranges.insert(range.clone());
        sections.insert(range.start, (range, names));
    }

    sections
}
