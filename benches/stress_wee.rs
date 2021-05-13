//! Benchmark for `wee_alloc` (which only supports being a global allocator)
//!
//! Specify `WEE_ALLOC_STATIC_ARRAY_BACKEND_BYTES=65536` by an environmental
//! variable.
//!
//! TODO: get this working, wee_alloc behaves too odd
#![no_std]
#![feature(const_maybe_uninit_assume_init)]
#![feature(slice_ptr_len)]
#![feature(default_alloc_error_handler)]
// TODO: Get rid of this conditional attribute; it's FarCri.rs's
//       implementation detail
#![cfg_attr(target_os = "none", no_main)]

extern crate alloc;

use core::ptr::NonNull;
use farcri::{criterion_group, criterion_main, Criterion};

mod stress_common;
use self::stress_common::bench_one_by_external_heap;

#[cfg(target_os = "none")]
#[global_allocator]
static _ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("noop", |b| b.iter(noop));

    bench_one_by_external_heap(
        c,
        "wee_alloc",
        |layout| unsafe { NonNull::new(alloc::alloc::alloc(layout)) },
        |p, layout| unsafe { alloc::alloc::dealloc(p.as_ptr(), layout) },
    );
}

#[inline(never)]
fn noop() {}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
