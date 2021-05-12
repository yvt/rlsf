//! This module is shared by `stress(?!_common)(_.*)` benchmark tests
use core::{alloc::Layout, mem::MaybeUninit, ptr::NonNull};
use farcri::{BenchmarkId, Criterion};

static mut ARENA: [MaybeUninit<u8>; 1024 * 50] = unsafe { MaybeUninit::uninit().assume_init() };
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
    mut init: impl FnMut(*mut [MaybeUninit<u8>]) -> T,
    mut alloc: impl FnMut(&mut T, Layout) -> NonNull<u8>,
    mut dealloc: impl FnMut(&mut T, NonNull<u8>, Layout),
) {
    let mut group = c.benchmark_group(name);
    let arena = unsafe { &mut ARENA };
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
        let num_allocs = (arena.len() / size_range.end / 2).min(allocs.len());
        let allocs = &mut allocs[..num_allocs];

        let mut state = init(arena);

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
