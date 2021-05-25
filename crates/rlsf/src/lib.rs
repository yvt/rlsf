//! This crate implements the TLSF (Two-Level Segregated Fit) dynamic memory
//! allocation algorithm¹. Requires Rust 1.51.0 or later.
//!
//!  - **Allocation and deallocation operations are guaranteed to complete in
//!    constant time.** TLSF is suitable for real-time applications.
//!
//!  - **Fast and small.** You can have both. It was found to be smaller and
//!    faster² than most `no_std`-compatible allocator crates.
//!
//!  - **The memory pool is provided by an application.** Examples of potential
//!    memory pool sources include: a `static` array for global memory
//!    allocation, a memory block allocated by another memory allocator for
//!    arena allocation.
//!
//!  - **This crate supports `#![no_std]`.** It can be used in bare-metal and
//!    RTOS-based applications.
//!
//! <!-- <small> doesn't work on GitHub -->
//!
//! <sub>¹ M. Masmano, I. Ripoll, A. Crespo and J. Real, "TLSF: a new dynamic
//! memory allocator for real-time systems," *Proceedings. 16th Euromicro
//! Conference on Real-Time Systems*, 2004. ECRTS 2004., Catania, Italy, 2004,
//! pp. 79-88, doi: 10.1109/EMRTS.2004.1311009.</sub>
//!
//! <sub>² Compiled for and measured on a STM32F401 microcontroller using
//! <a href="https://github.com/yvt/farcri-rs">FarCri.rs</a>.</sub>
//!
//! # Measured Performance
//!
//! ![The result of latency measurement on STM32F401 is shown here. rlsf:
//! 260–320 cycles. buddy-alloc: 340–440 cycles. umm_malloc: 300–700 cycles.
//! dlmalloc: 450–750 cycles.
//! ](https://ipfs.io/ipfs/QmaQExwdRAT4Z2LRsyJYft1QCMCroEZBJRzpLRMu5eNrLu/time-cm4f-xf-3.svg)
//!
//! <!-- `wee_alloc` could not be measured because it ran out of memory too
//! early, probably because of <https://github.com/rustwasm/wee_alloc/issues/85>
//! `umm_malloc` does not support specifying larger alignment values. -->
//!
//! ![The result of code size measurement on WebAssembly is shown here. rlsf:
//! 1267 bytes, rlsf + pool coalescing: 1584 bytes, wee_alloc: 1910 bytes,
//! dlmalloc: 9613 bytes.
//! ](https://ipfs.io/ipfs/QmREbCr4pXZuMxtFoKU1qXkvqbuLemiydZs8iMAGuVDtAk/size-wasm-xf.svg)
//!
//! <!-- The latest version at the point of writing was used for each library's
//! measurement. The exception is `wee_alloc`, for which a fork based on commit
//! f26c431df6f was used to make it compile on the latest nightly compiler. -->
//!
//! # Limitations
//!
//!  - **It does not support concurrent access.** A whole pool must be locked
//!    for allocation and deallocation. If you use a FIFO lock to protect the
//!    pool, the worst-case execution time will be `O(num_contending_threads)`.
//!    You should consider using a thread-caching memory allocator (e.g.,
//!    TCMalloc, jemalloc) if achieving a maximal throughput in a highly
//!    concurrent environment is desired.
//!
//!  - **Free blocks cannot be returned to the underlying memory system
//!    efficiently.**
//!
//! # Examples
//!
//! ## `Tlsf`: Core API
//!
//! ```rust
//! use rlsf::Tlsf;
//! use std::{mem::MaybeUninit, alloc::Layout};
//!
//! let mut pool = [MaybeUninit::uninit(); 65536];
//!
//! // On 32-bit systems, the maximum block size is 16 << FLLEN = 65536 bytes.
//! // The worst-case fragmentation is (16 << FLLEN) / SLLEN - 2 = 4094 bytes.
//! // `'pool` represents the memory pool's lifetime (`pool` in this case).
//! let mut tlsf: Tlsf<'_, u16, u16, (), 12, 16> = Tlsf::INIT;
//! //                 ^^                ^^  ^^
//! //                  |                 |  |
//! //                'pool               |  SLLEN
//! //                                   FLLEN
//! tlsf.insert_free_block(&mut pool);
//!
//! unsafe {
//!     let mut ptr1 = tlsf.allocate(Layout::new::<u64>()).unwrap().cast::<u64>();
//!     let mut ptr2 = tlsf.allocate(Layout::new::<u64>()).unwrap().cast::<u64>();
//!     *ptr1.as_mut() = 42;
//!     *ptr2.as_mut() = 56;
//!     assert_eq!(*ptr1.as_ref(), 42);
//!     assert_eq!(*ptr2.as_ref(), 56);
//!     tlsf.deallocate(ptr1.cast(), Layout::new::<u64>().align());
//!     tlsf.deallocate(ptr2.cast(), Layout::new::<u64>().align());
//! }
//! ```
//!
//! ## `GlobalTlsf`: Global Allocator
//!
//! ```rust
//! #[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
//! #[global_allocator]
//! static A: rlsf::SmallGlobalTlsf = rlsf::SmallGlobalTlsf::INIT;
//!
//! let mut m = std::collections::HashMap::new();
//! m.insert(1, 2);
//! m.insert(5, 3);
//! drop(m);
//! ```
//!
//! # Details
//!
//! ## Changes from the Original Algorithm
//!
//!  - The end of each memory pool is capped by a sentinel block
//!    (a permanently occupied block) instead of a normal block with a
//!    last-block-in-pool flag. This simplifies the code a bit and improves
//!    its worst-case performance and code size.
//!
#![no_std]
#![cfg_attr(feature = "doc_cfg", feature(doc_cfg))]

// FIXME: panicking in constants is unstable
macro_rules! const_panic {
    ($($tt:tt)*) => {
        #[allow(unconditional_panic)]
        {
            let _ = 1 / 0;
            loop {}
        }
    };
}

mod flex;
mod init;
pub mod int;
mod tlsf;
mod utils;
pub use self::{
    flex::*,
    init::*,
    tlsf::{Tlsf, TlsfOptions, GRANULARITY},
};

/// Attaches `#[cfg(...)]` and `#[doc(cfg(...))]` to a given item definition
/// to conditionally compile it only when we have a `GlobalTlsf` implementation
/// for the current target.
macro_rules! if_supported_target {
    (
        $($tt:tt)*
    ) => {
        #[cfg(any(
            all(target_arch = "wasm32", not(target_feature = "atomics")),
            unix,
            doc,
        ))]
        #[cfg_attr(
            feature = "doc_cfg",
            doc(cfg(any(
                all(target_arch = "wasm32", not(target_feature = "atomics")),
                unix,
                // no `doc` here
            )))
        )]
        $($tt)*
    };
}

if_supported_target! { mod global; }
if_supported_target! { pub use self::global::*; }

#[cfg(any(test, feature = "std"))]
extern crate std;

#[cfg(test)]
mod tests;
