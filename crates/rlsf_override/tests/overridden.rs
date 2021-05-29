use rlsf_override as _;

#[test]
fn hashset() {
    let mut m = std::collections::HashSet::new();
    for i in 0..10000 {
        m.insert(i);
    }
}

/// Test `libbz2`, which uses `malloc` and `free` internally
#[test]
fn bz2() {
    use bzip2::read::{BzDecoder, BzEncoder};
    use bzip2::Compression;
    use std::io::prelude::*;

    let data = "Hello, World!".as_bytes();
    let compressor = BzEncoder::new(data, Compression::best());
    let mut decompressor = BzDecoder::new(compressor);

    let mut contents = String::new();
    decompressor.read_to_string(&mut contents).unwrap();
    assert_eq!(contents, "Hello, World!");
}
