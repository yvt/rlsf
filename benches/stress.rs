//! Benchmark for `rlsf` and some other allocators
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
use self::stress_common::{bench_one, ARENA};

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("noop", |b| b.iter(noop));

    bench_one(
        c,
        "rlsf",
        unsafe { ARENA.len() },
        #[allow(const_item_mutation)]
        |arena_len| {
            let mut tlsf: Tlsf<'_, u16, u16, 12, 16> = Tlsf::INIT;
            let arena = unsafe { &mut ARENA[..arena_len] };
            tlsf.insert_free_block(&mut *arena);
            tlsf
        },
        |tlsf, layout| tlsf.allocate(layout).unwrap(),
        |tlsf, p, layout| unsafe { tlsf.deallocate(p, layout.align()) },
    );

    bench_one(
        c,
        "linked_list_allocator",
        unsafe { ARENA.len() },
        #[allow(const_item_mutation)]
        |arena_len| {
            let mut heap = linked_list_allocator::Heap::empty();
            let arena = unsafe { &mut ARENA[..arena_len] };
            unsafe { heap.init(arena.as_ptr() as usize, arena.len()) };
            heap
        },
        |heap, layout| heap.allocate_first_fit(layout).unwrap(),
        |heap, p, layout| unsafe { heap.deallocate(p, layout) },
    );

    use buddy_alloc::buddy_alloc;
    bench_one(
        c,
        "buddy_alloc",
        unsafe { ARENA.len() },
        #[allow(const_item_mutation)]
        |arena_len| unsafe {
            let arena = &mut ARENA[..arena_len];
            buddy_alloc::BuddyAlloc::new(buddy_alloc::BuddyAllocParam::new(
                arena.as_ptr() as *const u8,
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
