//! This module is shared by `stress(?!_common)(_.*)` benchmark tests
use core::{alloc::Layout, mem::MaybeUninit, ptr::NonNull};
use farcri::{BenchmarkId, Criterion};

/// The default arena, which `bench_one`'s caller can choose to use
#[allow(dead_code)]
pub static mut ARENA: [MaybeUninit<u8>; 1024 * 70] = unsafe { MaybeUninit::uninit().assume_init() };

static mut ALLOCS: [(NonNull<u8>, Layout); 256] = [(NonNull::dangling(), unsafe {
    Layout::from_size_align_unchecked(0, 1)
}); 256];

struct Xorshift32(u32);

impl Xorshift32 {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
}

pub fn bench_one<T>(
    c: &mut Criterion,
    name: &str,
    arena_capacity: usize,
    mut init: impl FnMut(usize) -> T,
    mut alloc: impl FnMut(&mut T, Layout) -> NonNull<u8>,
    mut dealloc: impl FnMut(&mut T, NonNull<u8>, Layout),
) {
    let mut group = c.benchmark_group(name);
    let allocs = unsafe { &mut ALLOCS };

    for &(min_size, mask) in &[
        (1, 7),
        (1, 15),
        (1, 63),
        (1, 255),
        (16, 15),
        (16, 63),
        (16, 127),
        (64, 63),
        (64, 127),
        (128, 127),
    ] {
        let size_range = min_size..min_size + mask + 1;
        let num_allocs = (arena_capacity / (size_range.end + 8) / 2).min(allocs.len());
        let allocs = &mut allocs[..num_allocs];

        let mut state = init(arena_capacity);

        let mut rng = Xorshift32(0x12345689);
        let mut next_layout = || {
            let len = (rng.next() as usize & mask) + min_size;
            let align = 4 << (rng.next() & 3);
            unsafe { Layout::from_size_align_unchecked(len, align) }
        };

        // Fill `allocs`
        for al in allocs.iter_mut() {
            let layout = next_layout();
            let p = alloc(&mut state, layout);
            *al = (p, layout);
        }

        group.bench_function(
            BenchmarkId::from_parameter(&format_args!("size {:?}", size_range)),
            |b| {
                let mut alloc_i = 0;
                b.iter(|| {
                    // deallocate
                    let (p, layout) = allocs[alloc_i & (allocs.len() - 1)];
                    dealloc(&mut state, p, layout);

                    // allocate
                    let layout = next_layout();
                    let p = alloc(&mut state, layout);
                    allocs[alloc_i & (allocs.len() - 1)] = (p, layout);

                    alloc_i = alloc_i.wrapping_add(1);
                });
            },
        );

        // Deallocate
        for &(p, layout) in allocs.iter() {
            dealloc(&mut state, p, layout);
        }
    }
}

/// Like `bench_one`, but uses an external unknown-sized heap (such as a
/// global allocator).
#[allow(dead_code)]
pub fn bench_one_by_external_heap(
    c: &mut Criterion,
    name: &str,

    mut alloc: impl FnMut(Layout) -> Option<NonNull<u8>>,
    mut dealloc: impl FnMut(NonNull<u8>, Layout),
) {
    // Determine the capacity of the global heap
    let arena_capacity = (1..24)
        .map(|i| 1 << i)
        .take_while(|&size| {
            let layout = Layout::from_size_align(size, 1).unwrap();
            let p = alloc(layout);
            log::debug!("probing arena size {} → {:?}", size, p);
            if let Some(p) = p {
                dealloc(p, layout);
                true
            } else {
                false
            }
        })
        .last()
        .unwrap();

    log::info!("arena_capacity = {}", arena_capacity);

    bench_one(
        c,
        name,
        arena_capacity,
        |arena_size| {
            assert!(arena_size == arena_capacity);
        },
        |_, layout| alloc(layout).unwrap(),
        |_, p, layout| dealloc(p, layout),
    );
}
