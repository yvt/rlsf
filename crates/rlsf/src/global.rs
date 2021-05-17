use core::{
    alloc,
    cell::UnsafeCell,
    ops,
    ptr::{self, NonNull},
};

use super::{tlsf::USIZE_BITS, FlexTlsf, Init};

// `doc(cfg(...))` needs to be attached to the type for it to be displayed
// on the docs.
if_supported_target! {
    /// [`Tlsf`] as a global allocator.
    ///
    /// [`Tlsf`]: crate::Tlsf
    pub struct GlobalTlsf {
        inner: UnsafeCell<TheTlsf>,
        #[cfg(not(doc))]
        mutex: os::Mutex,
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm32;
#[cfg(target_arch = "wasm32")]
use self::wasm32 as os;

#[cfg(doc)]
type TheTlsf = ();
#[cfg(not(doc))]
type TheTlsf = FlexTlsf<os::Source, usize, usize, { USIZE_BITS as usize }, { USIZE_BITS as usize }>;

impl Init for GlobalTlsf {
    const INIT: Self = Self::INIT;
}

unsafe impl Send for GlobalTlsf {}
unsafe impl Sync for GlobalTlsf {}

impl GlobalTlsf {
    /// The initializer.
    pub const INIT: Self = Self {
        inner: UnsafeCell::new(Init::INIT),
        mutex: Init::INIT,
    };
}

impl GlobalTlsf {
    #[inline]
    fn lock_inner(&self) -> impl ops::DerefMut<Target = TheTlsf> + '_ {
        struct LockGuard<'a>(&'a GlobalTlsf);

        impl ops::Deref for LockGuard<'_> {
            type Target = TheTlsf;

            #[inline]
            fn deref(&self) -> &Self::Target {
                // Safety: Protected by `mutex`
                unsafe { &*self.0.inner.get() }
            }
        }

        impl ops::DerefMut for LockGuard<'_> {
            #[inline]
            fn deref_mut(&mut self) -> &mut Self::Target {
                // Safety: Protected by `mutex`
                unsafe { &mut *self.0.inner.get() }
            }
        }

        impl Drop for LockGuard<'_> {
            #[inline]
            fn drop(&mut self) {
                self.0.mutex.unlock();
            }
        }

        self.mutex.lock();
        LockGuard(self)
    }
}

unsafe impl alloc::GlobalAlloc for GlobalTlsf {
    #[inline]
    unsafe fn alloc(&self, layout: alloc::Layout) -> *mut u8 {
        let mut inner = self.lock_inner();
        inner
            .allocate(layout)
            .map(NonNull::as_ptr)
            .unwrap_or(ptr::null_mut())
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: alloc::Layout) {
        let mut inner = self.lock_inner();
        // Safety: All allocations are non-null
        let ptr = NonNull::new_unchecked(ptr);
        // Safety: `ptr` denotes a previous allocation with alignment
        //         `layout.align()`
        inner.deallocate(ptr, layout.align());
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: alloc::Layout, new_size: usize) -> *mut u8 {
        let mut inner = self.lock_inner();
        // Safety: All allocations are non-null
        let ptr = NonNull::new_unchecked(ptr);
        // Safety: `layout.align()` is a power of two, and the size parameter's
        //         validity is upheld by the caller
        let new_layout = alloc::Layout::from_size_align_unchecked(new_size, layout.align());
        // Safety: `ptr` denotes a previous allocation with alignment
        //         `layout.align()`
        inner
            .reallocate(ptr, new_layout)
            .map(NonNull::as_ptr)
            .unwrap_or(ptr::null_mut())
    }
}
