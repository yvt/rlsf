//! This crate implements the TLSF (Two-Level Segregated Fit) dynamic memory
//! allocation algorithm¹.
//!
//!  - **Allocation and deallocation operations are guaranteed to complete in
//!    constant time.** TLSF is suitable for real-time applications.
//!
//!  - **Fast and small.** You can have both. It was found to be smaller and
//!    faster² than three randomly chosen `no_std`-compatible allocator crates.
//!
//!  - **The memory pool is provided by an application³.** Examples of potential
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
//! <sub>³ But rlsf can't return free memory blocks to the underlying memory
//! system. If that's a problem, you should just use the default allocator
//! (and keep the I-cache clean).
//! </sub>
//!
//! # Examples
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
//! let mut tlsf: Tlsf<'_, u16, u16, 12, 16> = Tlsf::INIT;
//! //                 ^^            ^^  ^^
//! //                  |             |  |
//! //                'pool           |  SLLEN
//! //                               FLLEN
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
#![no_std]

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

pub mod int;
mod tlsf;
pub use self::tlsf::{Tlsf, GRANULARITY};

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
