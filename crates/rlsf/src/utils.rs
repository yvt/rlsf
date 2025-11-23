use core::{cell::UnsafeCell, mem::MaybeUninit, ptr::NonNull};

/// Polyfill for <https://github.com/rust-lang/rust/issues/71941>
#[inline]
pub fn nonnull_slice_from_raw_parts<T>(ptr: NonNull<T>, len: usize) -> NonNull<[T]> {
    unsafe { NonNull::new_unchecked(core::ptr::slice_from_raw_parts_mut(ptr.as_ptr(), len)) }
}

/// Polyfill for  <https://github.com/rust-lang/rust/issues/71146>
///
/// # Safety
///
/// `ptr` must be dereferencable. This is a limitation of the polyfill.
#[inline]
pub unsafe fn nonnull_slice_len<T>(ptr: NonNull<[T]>) -> usize {
    // FIXME: Use `NonNull<[T]>::len` (stabilized in Rust 1.63)
    // Safety: We are just reading the slice length embedded in the fat
    //         pointer and not dereferencing the pointer. We also convert it
    //         to `*mut [MaybeUninit<UnsafeCell<u8>>]` just in case because the
    //         slice might be uninitialized and there might be outstanding
    //         mutable references to the slice.
    (&*(ptr.as_ptr() as *const [MaybeUninit<UnsafeCell<T>>])).len()
}

// Polyfill for <https://github.com/rust-lang/rust/issues/74265>
#[inline]
pub fn nonnull_slice_start<T>(ptr: NonNull<[T]>) -> NonNull<T> {
    unsafe { NonNull::new_unchecked(ptr.as_ptr() as *mut T) }
}

/// Get the one-past-end pointer of a slice pointer.
///
/// # Safety
///
/// `ptr` must be dereferencable. This is a limitation of [`nonnull_slice_len`].
#[inline]
pub unsafe fn nonnull_slice_end<T>(ptr: NonNull<[T]>) -> *mut T {
    (ptr.as_ptr() as *mut T).wrapping_add(nonnull_slice_len(ptr))
}
