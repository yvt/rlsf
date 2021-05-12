#![no_std]
#![feature(const_maybe_uninit_assume_init)]
#![feature(slice_ptr_len)]
// TODO: Get rid of this conditional attribute; it's FarCri.rs's
//       implementation detail
#![cfg_attr(target_os = "none", no_main)]

use core::{alloc::Layout, mem::MaybeUninit, ptr::NonNull};
use farcri::{criterion_group, criterion_main, BenchmarkId, Criterion};
use rlsf::Tlsf;

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

fn bench_one<T>(
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
    }
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("noop", |b| b.iter(noop));

    bench_one(
        c,
        "rlsf",
        #[allow(const_item_mutation)]
        |arena| {
            let mut tlsf: Tlsf<'_, u16, u16, 12, 16> = Tlsf::INIT;
            tlsf.insert_free_block(unsafe { &mut *arena });
            tlsf
        },
        |tlsf, layout| tlsf.allocate(layout).unwrap(),
        |tlsf, p, layout| unsafe { tlsf.deallocate(p, layout.align()) },
    );

    bench_one(
        c,
        "linked_list_allocator",
        #[allow(const_item_mutation)]
        |arena| {
            let mut heap = linked_list_allocator::Heap::empty();
            unsafe { heap.init(arena as *mut u8 as usize, arena.len()) };
            heap
        },
        |heap, layout| heap.allocate_first_fit(layout).unwrap(),
        |heap, p, layout| unsafe { heap.deallocate(p, layout) },
    );

    use buddy_alloc::buddy_alloc;
    bench_one(
        c,
        "buddy_alloc",
        #[allow(const_item_mutation)]
        |arena| unsafe {
            buddy_alloc::BuddyAlloc::new(buddy_alloc::BuddyAllocParam::new(
                arena as *const u8,
                arena.len(),
                16,
            ))
        },
        |heap, layout| NonNull::new(heap.malloc(layout.size())).unwrap(),
        |heap, p, _| heap.free(p.as_ptr()),
    );
}

#[inline(never)]
fn noop() {}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
