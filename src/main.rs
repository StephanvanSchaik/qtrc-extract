mod name;
mod tree;

use anyhow::{Context, Result};
use clap::Parser;
use rangemap::RangeSet;
use std::collections::HashSet;
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
        println!("Found set of names at 0x{:x}...", range.start);

        let tree_offsets = tree::find_tree_offsets(names, &bytes);

        for tree_offset in tree_offsets {
            println!("Found file tree at 0x{:x}...", tree_offset);

            let mut blob_offsets = tree::find_blob_offsets(tree_offset, &bytes);

            if blob_offsets.is_empty() {
                blob_offsets.insert(((range.end + 15) & !15));
            }

            for blob_offset in blob_offsets {
                println!("Found data blobs at 0x{:x}...", blob_offset);
                println!("Extracting file tree...");

                tree::extract_tree(&output, names, &bytes[blob_offset..], &bytes[tree_offset..], 0, 1);
            }
        }
    }

    Ok(())
}