#![no_std]
#![feature(const_maybe_uninit_assume_init)]
#![feature(slice_ptr_len)]
// TODO: Get rid of this conditional attribute; it's FarCri.rs's
//       implementation detail
#![cfg_attr(target_os = "none", no_main)]

use core::ptr::NonNull;
use farcri::{criterion_group, criterion_main, Criterion};
use rlsf::Tlsf;

mod stress_common;
use self::stress_common::bench_one;

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
