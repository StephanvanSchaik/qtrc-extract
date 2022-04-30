mod name;
mod tree;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

use crate::name::scan_names;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    input: String,

    #[clap(short, long)]
    output: Option<String>,
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

    for (range, names) in names.iter() {
        println!("Found set of names at 0x{:x}-0x{:x}...", range.start, range.end);

        let tree_offsets = tree::find_tree_offsets(names, &bytes);

        for (tree_offset, tree_size) in tree_offsets {
            println!("Found file tree at 0x{:x}-0x{:x}...", tree_offset, tree_offset + tree_size);

            let mut blob_offsets = tree::find_blob_offsets(tree_offset, &bytes);

            // FIXME: add the Windows version.
            if blob_offsets.is_empty() {
                // Align the offset to 8 bytes.
                let mut offset = (range.end + 7) & !7;

                // Skip 8 bytes of padding until we find no more padding.
                while offset + 8 <= bytes.len() && bytes[offset..][..8].iter().all(|c| *c == 0) {
                    offset += 8;
                }

                // If we did not reach the end of the file, then we probably found a good blob
                // offset.
                if offset + 8 <= bytes.len() {
                    blob_offsets.insert(offset);
                }
            }

            for blob_offset in blob_offsets {
                println!("Found data blobs at 0x{:x}...", blob_offset);
                println!("Extracting file tree...");

                tree::extract_tree(&output, names, &bytes[blob_offset..], &bytes[tree_offset..], 0, 1)?;
            }
        }
    }

    Ok(())
}
