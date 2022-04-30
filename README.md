# qtrc-extract

A tool written in Rust to extract resources from executables that use Qt.

## Features

* [x] Support for zlib compressed blobs.
* [x] Automatically finds the tree, blob and name offsets.
* [ ] Support for zstd compressed blobs (due for Qt 6).
* [ ] Support for version 1 of the file format (which lacks modified timestamps).

This needs a little bit more work to figure out padding and visited ranges, but should already work quite well otherwise.

## Usage

Build qtrc-extract as follows:

```
cargo build --release
```

Then you can run it on `some-executable.exe` storing the output to `output` as follows:

```
cargo run --release -- some-executable.exe --output=output
```

## How does this work?

Applications that use Qt to store their resources can store one or more trees that describe a hierarchy of directory and files.
The information/metadata for each tree is split up over three sections:

 * The **blob** section contains the actual file blobs without any metadata other than the (compressed) size.
 * The **tree** section contains tree entries that each either describe a directory or a file.
 * The **name** section contains UTF-16 strings with a 16-bit size and a 32-bit hash.

The sections are usually found sequentially in the executable, possibly with some padding in between.
However, after some experimentation I found out that the order of the sections it not always the same:

* Microsoft Windows order: \<blobs\> \<names\> \<tree\>
* Linux order: \<tree\> \<names\> \<blobs\>

In addition, `rcc` will only compress resources if the compression is actually helpful.
Therefore, any given executable may have trees that are completely uncompressed, or just contain a single file.
This means that on top of the metadata not having any signatures, you may not have any zlib signatures to work with either.

Instead qtrc-extract relies on heuristics to locate Qt resource trees.
First, note that a name entry looks as follows:

* size: unsigned 16-bit integer
* hash: unsigned 32-bit integer
* name: UTF-16 string

Also note that all metadata is in **big endian**.
Since we are dealing with file names, it is very likely that file names will not use characters outside the ASCII range.
Therefore, we can just look for byte pairs that look like 0x00 0x?? where ?? falls within the printable/graphical ASCII range.
For every such sequence, we can then assume that the size and the hash fields precedes it and verify the actual string against the actual size and hash.

For reference, the hash function used looks as follows:

```
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
```

Once we have at least one name entry, we can simply try decoding sequential name entries until we hit data that does not represent a name entry, which will very likely produce an incorrect hash.
This means that we generally just need the first name entry of a set of names to be within the ASCII range.

Next we want to find the **tree** section describing the actual resource tree.
As mentioned before a tree entry can either be a file or a directory.
A directory entry looks as follows:

* name\_offset: unsigned 32-bit integer
* flags: unsigned 16-bit integer
* count: unsigned 32-bit integer
* node\_id: unsigned 32-bit integer
* last\_modified: unsigned 64-bit integer (since version 2)

Whereas a file entry looks as follows:

* name\_offset: unsigned 32-bit integer
* flags: unsigned 16-bit integer
* locale: unsigned 32-bit integer
* data\_offset: unsigned 32-bit integer
* last\_modified: unsigned 64-bit integer (since version 2)

Once we know where the tree section is, we can recursively iterate over its entries starting with a single directory/file entry.
Whenever we find a directory entry, its node ID * entry size in bytes gives us the offset into the tree section, whereas the count will tell us how many entries belong to the directory.
As this is a tree, this means there can be no loops and directories therefore must always contain their own unique set of entries.

In addition, each entry has a name offset which is a relative offset into the name section.
Since we know where the name section is, we can just calculate the offset of each name relative to the start of the name section and check that each entry references a valid name.
Furthermore, we expect the executable not to have any unused names, so we would expect to see all name entries being referenced to at least once.

These heuristics allow us to find the corresponding tree section for the name section.

Finally, to find the blob section, we note that the file entries in the tree section contain a data offset.
This offset is again relative to the start of the blob section.

Each blob looks as follows:

* size: unsigned 32-bit integer
* payload: size number of bytes

In addition, blobs in the blob sections are sequential.
Therefore, we can find the next blob by adding the size of the current blob + 4 bytes for the 32-bit size field to the offset.
Thus, to find the blob section, we collect all the data offsets from the file entries, sort them in ascending order, and calculate the deltas between every two offsets (- 4 to account for the 32-bit size field).
Then we try to find the first blob by looking for the first delta, and then simply traverse the chain of blobs confirming each blob's size with the deltas we found.

Unfortunately, this approach has one drawback, which is that some resource trees may only contain one file and therefore only one data offset.
Thus leaving us without a way to figure out the size of a single blob.
However, note that these resource trees usually follow a particular layout where the blobs either succeed the names (Linux) or where the blobs preceed the names (Windows).
This means that we can look for the blob starting from the end of the name section (Linux) or look for the blob in reverse starting from the start of the name section (Windows).

In general this approach works pretty well to extract resources from executables that use Qt's resource packing, but can be further improved by looking for specific file headers (e.g. zlib signatures, PNG singatures, GIF signatures).
Ideally, this should be implemented in such a way that the user of this tool can simply specify their own file containing the signatures per line using a format like "CA ?? FE".

## Similar Projects

* [extract-qt-resources](https://github.com/dgchurchill/extract-qt-resources) is written in F# and requires Mono on Linux. I wasn't able to get this one to compile, since I have very little experience with Mono, but this seemed to be the most promising judging it by its parsers.
* [qrc](https://github.com/pgaskin/qrc) is written in Go and requires the user to specify offsets to the tree, names and blobs section in order to extract them from an executable.
* [qtextract](https://github.com/axstin/qtextract) is written in Lua and seems to look for specific sequences in the code of the executable to locate the offsets. This didn't seem to work for me, which may be due to the specific code sequences for which it is looking.
* [qresExtract](https://github.com/tatokis/qresExtract) is written in C++ and seems to be a simple parser for rcc files, but does not seem to have any logic to extract Qt resources from executables.

## References

* https://github.com/qt/qtbase/blob/dev/src/tools/rcc/rcc.cpp
