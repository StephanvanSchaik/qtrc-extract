# qtrc-extract

A tool written in Rust to extract resources from executables that use Qt.

## Features

* [x] Support for zlib compressed blobs.
* [x] Automatically finds the tree, blob and name offsets.
* [ ] Support for zstd compressed blobs (due for Qt 6).
* [ ] Support for version 1 of the file format (which lacks modified timestamps).

## Usage

To build qtrc-extract you need both Git and [Rust](https://rustup.rs).
Then using Git you can check out this repository as follows:

```
git clone https://github.com/StephanvanSchaik/qtrc-extract
```

Then navigate to the qtrc-extract directory and build qtrc-extract as follows:

```
cargo build --release
```

Then you can run it on `some-executable.exe` storing the output to `output` as follows:

```
cargo run --release -- some-executable.exe --output=output
```

Alternatively, you can run the tool as follows:

```
./target/release/qtrc-extract some-executable.exe --output=output
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
To remediate this, we instead rely on the fact that the tree and file offset we know are likely to be correct, and more so that we can use this information to find the possible call sites to `qRegisterResourceData`.

More specifically, when the application registers Qt resources, it passes the version, the tree offset, the name offset and the blob offset as the arguments to that function.
However, the application will be passing the offsets as virtual addresses pointing to where the Qt resources are located in the virtual address space, rather than file offsets to where the Qt resources are located within the executable file.
Therefore, we must first parse the PE sections/ELF program headers to establish a mapping between the virtual address space and the file offsets.
We can then use this information to translate the tree offsets and name offsets we found to their counterparts in the virtual address space.
Then, for x86 applications at least, we can simply look for these constants in the executable and find `PUSH` instructions that directly push this 32-bit constants.
More specifically, these will be of the form 68 XX XX XX XX where XX XX XX XX is the 32-bit address.

For x86-64 applications, things are slightly more complicated as a) the calling conventions on x86-64 use registers for the first few arguments and B) as it is very likely that the application uses `LEA RDX, [RIP + 0xXXXXXXXX]` to load the constants into the registers.
To calculate the actual value to look for in the binary we have to add the instruction offset of the instruction **after** the `LEA` instruction to the constant being added to the `RIP` register.
In addition, we can look for the opcode 8D (LEA) as well as the specific destination register to match the constant with the appropriate argument.

Once we know where the instructions referencing the name offset and tree offset are located within our program, we can simply look for the closest `LEA` instruction that targets the appropriate destination register for the blob offset to find the blob offset.
Similarly, we can look for the closest `PUSH` instruction before the `PUSH` using the name/tree offset to find the blob offset, as the `PUSH` instructions have to be present in the reverse order of the arguments anyway.
Since the blob offset is actually a virtual address, we have to perform some calculations to get the actual file offset.

Of course, as we are relying on heuristics to locate Qt resources, these techniques and as a result qtrc-extract is not guaranteed to work for every possible executable, and sometimes reverse engineering is inevitable.
However, understanding the heuristics and techniques used by qtrc-extract helps in understanding where to look in the case you have to reverse engineer such a binary yourself.
In most other cases, this tool will be able to extract Qt resources completely automatically for you.

## Similar Projects

* [extract-qt-resources](https://github.com/dgchurchill/extract-qt-resources) is written in F# and requires Mono on Linux. I wasn't able to get this one to compile, since I have very little experience with Mono, but this seemed to be the most promising judging it by its parsers.
* [qrc](https://github.com/pgaskin/qrc) is written in Go and requires the user to specify offsets to the tree, names and blobs section in order to extract them from an executable.
* [qtextract](https://github.com/axstin/qtextract) is written in Lua and seems to look for specific sequences in the code of the executable to locate the offsets. Unfortunately, this is not very reliable as it depends on the generated code within the executable.
* [qresExtract](https://github.com/tatokis/qresExtract) is written in C++ and seems to be a simple parser for rcc files, but does not seem to have any logic to extract Qt resources from executables.

## References

* https://github.com/qt/qtbase/blob/dev/src/tools/rcc/rcc.cpp
