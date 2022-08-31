// Adopted from
// https://github.com/alexcrichton/dlmalloc-rs/blob/master/tests/global.rs
use std::{
    alloc::{GlobalAlloc, Layout},
    collections::HashMap,
};

#[global_allocator]
#[cfg(any(all(target_arch = "wasm32", not(target_feature = "atomics")), unix))]
static A: rlsf::SmallGlobalTlsf = rlsf::SmallGlobalTlsf::new();

#[test]
fn foo() {
    println!("hello");
}

#[test]
fn map() {
    let mut m = HashMap::new();
    m.insert(1, 2);
    m.insert(5, 3);
    drop(m);
}

#[test]
fn strings() {
    format!("foo, bar, {}", "baz");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn threads() {
    assert!(std::thread::spawn(|| panic!()).join().is_err());
}

#[test]
fn test_larger_than_word_alignment() {
    use std::mem;

    // Align to 32 bytes.
    #[repr(align(32))]
    struct Align32(u8);

    assert_eq!(mem::align_of::<Align32>(), 32);

    for _ in 0..1000 {
        let b = Box::new(Align32(42));

        let p = Box::into_raw(b);
        assert_eq!(p as usize % 32, 0, "{:p} should be aligned to 32", p);

        unsafe {
            let b = Box::from_raw(p);
            assert_eq!(b.0, 42);
        }
    }
}

#[test]
fn cannot_alloc_max_usize_minus_some() {
    // The test should complete without causing OOM
    for offset in (0..64).step_by(8) {
        let layout = Layout::from_size_align(usize::MAX - offset, 1).unwrap();
        for _ in 0..1000000 {
            let result = unsafe { A.alloc(layout) };
            assert!(result.is_null());
        }
    }
}
