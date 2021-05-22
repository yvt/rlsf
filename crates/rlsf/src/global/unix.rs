use crate::{utils::nonnull_slice_len, Init};
use core::{marker::PhantomData, ptr::NonNull};

use super::GlobalTlsfOptions;

const MIN_ALIGN: usize = crate::GRANULARITY;

/// The allocation unit, which is intentionally set to be larger than the usual
/// page sizes to reduce overhead. TODO: Make this adjustable
const ALLOC_UNIT: usize = 1 << 16;

pub struct Mutex(());

impl Init for Mutex {
    const INIT: Self = Self(());
}

/// `pthread_mutex_t` might be unsafe to move, so we can't put it in `Mutex`.
static mut MUTEX: libc::pthread_mutex_t = libc::PTHREAD_MUTEX_INITIALIZER;

impl Mutex {
    #[inline]
    pub fn lock(&self) {
        unsafe {
            libc::pthread_mutex_lock(&mut MUTEX);
            if PAGE_SIZE_M1 == 0 {
                init_page_size();
            }
        }
    }

    #[inline]
    pub fn unlock(&self) {
        unsafe { libc::pthread_mutex_unlock(&mut MUTEX) };
    }
}

pub struct Source<Options>(PhantomData<fn() -> Options>);

impl<Options> Init for Source<Options> {
    const INIT: Self = Self(PhantomData);
}

/// The memory page size minus 1. Set by `Mutex::lock`.
static mut PAGE_SIZE_M1: usize = 0;
#[cold]
fn init_page_size() {
    unsafe {
        let page_size = (libc::sysconf(libc::_SC_PAGESIZE) as usize).max(ALLOC_UNIT);
        if !page_size.is_power_of_two() {
            libc::abort();
        }
        PAGE_SIZE_M1 = page_size - 1;

        // Such a small memory page size is quite unusual.
        if page_size < MIN_ALIGN {
            libc::abort();
        }
    }
}

unsafe impl<Options: GlobalTlsfOptions> crate::flex::FlexSource for Source<Options> {
    #[inline]
    unsafe fn alloc(&mut self, min_size: usize) -> Option<NonNull<[u8]>> {
        let num_bytes = min_size.checked_add(PAGE_SIZE_M1)? & !PAGE_SIZE_M1;

        let ptr = libc::mmap(
            0 as *mut _,
            num_bytes,
            libc::PROT_WRITE | libc::PROT_READ,
            libc::MAP_ANONYMOUS | libc::MAP_PRIVATE,
            -1,
            0,
        );

        if ptr == libc::MAP_FAILED {
            return None;
        }

        NonNull::new(core::ptr::slice_from_raw_parts_mut(
            ptr as *mut u8,
            num_bytes,
        ))
    }

    #[inline]
    unsafe fn realloc_inplace_grow(
        &mut self,
        ptr: NonNull<[u8]>,
        min_new_len: usize,
    ) -> Option<usize> {
        if !Options::COALESCE_POOLS {
            return None;
        }

        let num_bytes = min_new_len.checked_add(PAGE_SIZE_M1)? & !PAGE_SIZE_M1;

        let ptr_new = libc::mremap(ptr.as_ptr() as *mut _, nonnull_slice_len(ptr), num_bytes, 0);
        if ptr_new == libc::MAP_FAILED {
            None
        } else {
            debug_assert_eq!(ptr_new as *mut u8, ptr.as_ptr() as *mut u8);
            Some(num_bytes)
        }
    }

    #[inline]
    fn supports_realloc_inplace_grow(&self) -> bool {
        Options::COALESCE_POOLS
    }

    // Not implementing `dealloc` because there is no safe way to destruct
    // a registered global allocator anyway.

    #[inline]
    fn min_align(&self) -> usize {
        // Return a conservative yet enough-for-optimization constant number
        MIN_ALIGN
    }
}
