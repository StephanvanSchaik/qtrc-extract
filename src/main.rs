mod name;
mod tree;

use anyhow::{Context, Result};
use clap::Parser;
use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;

use crate::name::scan_names;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    input: String,

    #[clap(short, long)]
    output: Option<String>,
}

/// Calculates the distance between two ranges.
fn distance(lhs: &Range<usize>, rhs: &Range<usize>) -> usize {
    if lhs.end <= rhs.start {
        rhs.start - lhs.end
    } else if rhs.end <= lhs.start {
        lhs.start - rhs.end
    } else {
        0
    }
}

fn main() -> Result<()> {
    // Parse the arguments.
    let args = Args::parse();

    let output = args.output
        .map(|output| PathBuf::from(output))
        .unwrap_or(PathBuf::new());

    let bytes = std::fs::read(&args.input)
        .with_context(|| format!("could not open file '{}'.", &args.input))?;

    let names = scan_names(&bytes);

    for (_, (name_range, names)) in names.iter() {
        println!("Found set of names at 0x{:x}-0x{:x}...", name_range.start, name_range.end);

        let trees = tree::find_trees(names, &bytes);

        let trees: BTreeMap<usize, Range<usize>> = trees
            .into_iter()
            .map(|(_, tree_range)| (distance(name_range, &tree_range), tree_range))
            .collect();

        'outer: for (score, tree_range) in trees {
            println!("Found file tree at 0x{:x}-0x{:x} with proximity score {}...", tree_range.start, tree_range.end, score);

            let mut blobs = tree::find_blobs(tree_range.start, &bytes);

            // FIXME: add the Windows version.
            if blobs.is_empty() {
                // Align the offset to 8 bytes.
                let mut offset = (name_range.end + 7) & !7;

                // Skip 8 bytes of padding until we find no more padding.
                while offset + 8 <= bytes.len() && bytes[offset..][..8].iter().all(|c| *c == 0) {
                    offset += 8;
                }

                // If we did not reach the end of the file, then we probably found a good blob
                // offset.
                if offset + 8 <= bytes.len() {
                    // Decode the size field.
                    let mut slice = [0u8; 4];
                    slice.copy_from_slice(&bytes[offset..][..4]);
                    let size = u32::from_be_bytes(slice) as usize;

                    blobs.insert(offset, offset..offset + size + 4);
                }
            }

            let blobs: BTreeMap<usize, Range<usize>> = blobs
                .into_iter()
                .map(|(_, blob_range)| (distance(name_range, &blob_range), blob_range))
                .collect();

            for (score, blob_range) in blobs {
                println!("Found data blobs at 0x{:x}-0{:x} with proximity score {}...", blob_range.start, blob_range.end, score);
                println!("Extracting file tree...");

                if let Ok(()) = tree::extract_tree(&output, names, &bytes[blob_range.clone()], &bytes[tree_range.start..], 0, 1) {
                    break 'outer;
                }
            }
        }
    }

    Ok(())
}
