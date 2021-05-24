//! Benchmark for `rlsf` and some other allocators
#![no_std]
#![feature(const_maybe_uninit_assume_init)]
#![feature(slice_ptr_len)]
// TODO: Get rid of this conditional attribute; it's FarCri.rs's
//       implementation detail
#![cfg_attr(target_os = "none", no_main)]

use core::{cell::Cell, ptr::NonNull};
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
        "umm_malloc",
        unsafe { ARENA.len() },
        |arena_len| unsafe {
            umm_malloc_sys::umm_init(ARENA.as_mut_ptr() as _, arena_len);
        },
        |_allocator, layout| unsafe {
            // ignoring `layout.align()`
            NonNull::new(umm_malloc_sys::umm_malloc(layout.size()))
                .unwrap()
                .cast()
        },
        |_allocator, p, _layout| unsafe { umm_malloc_sys::umm_free(p.as_ptr() as _) },
    );
    return;

    bench_one(
        c,
        "linked_list_allocator",
        unsafe { ARENA.len() },
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

    bench_one(
        c,
        "dlmalloc",
        unsafe { ARENA.len() - PAGE_SIZE },
        #[allow(const_item_mutation)]
        |arena_len| unsafe {
            let arena = &mut ARENA[..arena_len];

            dlmalloc::Dlmalloc::new_with_allocator(DlBumpAllocator::new(arena))
        },
        |heap, layout| unsafe { NonNull::new(heap.malloc(layout.size(), layout.align())).unwrap() },
        |heap, p, layout| unsafe { heap.free(p.as_ptr(), layout.size(), layout.align()) },
    );
}

struct DlBumpAllocator {
    start_free: Cell<usize>,
    end: usize,
}

const PAGE_SIZE: usize = 4096;

impl DlBumpAllocator {
    unsafe fn new(arena: *mut [core::mem::MaybeUninit<u8>]) -> Self {
        let start = arena as *mut core::mem::MaybeUninit<u8> as usize;
        let end = start + arena.len();
        let start = (start + (PAGE_SIZE - 1)) & !(PAGE_SIZE - 1);
        assert!(start < end);
        Self {
            start_free: Cell::new(start),
            end,
        }
    }
}

unsafe impl dlmalloc::Allocator for DlBumpAllocator {
    fn alloc(&self, size: usize) -> (*mut u8, usize, u32) {
        let start_free = self.start_free.get();
        let new_start_free = start_free.checked_add(size).filter(|&x| x <= self.end);
        if let Some(new_start_free) = new_start_free {
            log::debug!(
                "DlBumpAllocator: allocated alloc({}) at {:?}",
                size,
                start_free,
            );
            self.start_free.set(new_start_free);
            (start_free as *mut u8, size, 0)
        } else {
            log::debug!(
                "DlBumpAllocator: alloc({}) failed; only {} bytes free",
                size,
                self.end - start_free,
            );
            (0 as _, 0, 0)
        }
    }
    fn remap(&self, _ptr: *mut u8, _oldsize: usize, _newsize: usize, _can_move: bool) -> *mut u8 {
        0 as _
    }
    fn free_part(&self, _ptr: *mut u8, _oldsize: usize, _newsize: usize) -> bool {
        false
    }
    fn free(&self, _ptr: *mut u8, _size: usize) -> bool {
        false
    }
    fn can_release_part(&self, _flags: u32) -> bool {
        false
    }
    fn allocates_zeros(&self) -> bool {
        true
    }
    fn page_size(&self) -> usize {
        PAGE_SIZE
    }
}

#[inline(never)]
fn noop() {}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
