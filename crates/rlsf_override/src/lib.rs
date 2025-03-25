//! Overrides C memory allocation functions with [`::rlsf`].
use rlsf::CAlloc;
use std::{
    alloc::Layout,
    os::raw::{c_int, c_void},
    ptr::{null_mut, NonNull},
};

#[global_allocator]
pub static ALLOC: rlsf::GlobalTlsf = rlsf::GlobalTlsf::new();

/// The alignment guaranteed by `malloc`.
const MIN_ALIGN: usize = match () {
    #[cfg(any(
        target_arch = "x86",
        target_arch = "arm",
        target_arch = "mips",
        target_arch = "powerpc",
        target_arch = "powerpc64",
        target_arch = "sparc",
        target_arch = "asmjs",
        target_arch = "wasm32",
        target_arch = "hexagon",
        target_arch = "riscv32"
    ))]
    () => 8,
    #[cfg(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "mips64",
        target_arch = "s390x",
        target_arch = "sparc64",
        target_arch = "riscv64"
    ))]
    () => 16,
};

#[no_mangle]
pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    aligned_alloc(MIN_ALIGN, size)
}

#[no_mangle]
pub unsafe extern "C" fn malloc_usable_size(p: *mut c_void) -> usize {
    if let Some(p) = NonNull::new(p) {
        CAlloc::allocation_usable_size(&ALLOC, p.cast())
    } else {
        0
    }
}

#[inline]
fn page_size() -> usize {
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
}

#[no_mangle]
pub unsafe extern "C" fn valloc(size: usize) -> *mut c_void {
    aligned_alloc(page_size(), size)
}

#[no_mangle]
pub unsafe extern "C" fn pvalloc(size: usize) -> *mut c_void {
    let page_size = page_size();
    if let Some(size) = size
        .checked_add(page_size - 1)
        .map(|x| x & !(page_size - 1))
    {
        aligned_alloc(page_size, size)
    } else {
        null_mut()
    }
}

#[no_mangle]
pub unsafe extern "C" fn calloc(number: usize, size: usize) -> *mut c_void {
    let layout = size
        .checked_mul(number)
        .and_then(|len| Layout::from_size_align(len, MIN_ALIGN).ok());
    if let Some((ptr, size)) =
        layout.and_then(|layout| CAlloc::allocate(&ALLOC, layout).map(|p| (p, layout.size())))
    {
        ptr.as_ptr().write_bytes(0, size);
        ptr.as_ptr() as *mut c_void
    } else {
        null_mut()
    }
}

#[no_mangle]
pub unsafe extern "C" fn posix_memalign(
    out_ptr: *mut *mut c_void,
    alignment: usize,
    size: usize,
) -> c_int {
    let ptr = aligned_alloc(alignment, size);
    *out_ptr = ptr as *mut c_void;
    if ptr.is_null() {
        libc::ENOMEM
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn aligned_alloc(alignment: usize, size: usize) -> *mut c_void {
    let layout = Layout::from_size_align(size, alignment).ok();
    if let Some(ptr) = layout.and_then(|layout| CAlloc::allocate(&ALLOC, layout)) {
        ptr.as_ptr() as *mut c_void
    } else {
        null_mut()
    }
}

#[no_mangle]
pub unsafe extern "C" fn memalign(alignment: usize, size: usize) -> *mut c_void {
    aligned_alloc(alignment, size)
}

#[no_mangle]
pub unsafe extern "C" fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    if let Some(ptr) = NonNull::new(ptr) {
        // `realloc` doesn't preserve the allocation's original alignment
        // <https://stackoverflow.com/a/9078627>
        Layout::from_size_align(size, MIN_ALIGN)
            .ok()
            .and_then(|layout| CAlloc::reallocate(&ALLOC, ptr.cast(), layout))
            .map(|ptr| ptr.as_ptr() as *mut c_void)
            .unwrap_or(null_mut())
    } else {
        malloc(size)
    }
}

#[no_mangle]
pub unsafe extern "C" fn free(ptr: *mut c_void) {
    if let Some(ptr) = NonNull::new(ptr) {
        CAlloc::deallocate(&ALLOC, ptr.cast());
    }
}

// TODO: Find a way to define these in a C++ source file and make sure the
//       symbols are exported by the final cdylib file
/// `operator delete[](void*, unsigned long, std::align_val_t)`
#[no_mangle]
pub unsafe extern "C" fn _ZdaPvmSt11align_val_t(p: *mut c_void, _: usize, _: usize) {
    free(p);
}

/// `operator delete[](void*, std::align_val_t, std::nothrow_t const&)`
#[no_mangle]
pub unsafe extern "C" fn _ZdaPvSt11align_val_tRKSt9nothrow_t(p: *mut c_void, _: usize, _: &c_void) {
    free(p);
}

/// `operator delete[](void*, std::align_val_t)`
#[no_mangle]
pub unsafe extern "C" fn _ZdaPvSt11align_val_t(p: *mut c_void, _: usize) {
    free(p);
}

/// `operator delete(void*, unsigned long, std::align_val_t)`
#[no_mangle]
pub unsafe extern "C" fn _ZdlPvmSt11align_val_t(p: *mut c_void, _: usize, _: usize) {
    free(p);
}

/// `operator delete(void*, std::align_val_t, std::nothrow_t const&)`
#[no_mangle]
pub unsafe extern "C" fn _ZdlPvSt11align_val_tRKSt9nothrow_t(p: *mut c_void, _: usize, _: usize) {
    free(p);
}

/// `operator delete(void*, std::align_val_t)`
#[no_mangle]
pub unsafe extern "C" fn _ZdlPvSt11align_val_t(p: *mut c_void, _: usize) {
    free(p);
}

/// `operator new[](unsigned long, std::align_val_t, std::nothrow_t const&)`
#[no_mangle]
pub unsafe extern "C" fn _ZnamSt11align_val_tRKSt9nothrow_t(
    size: usize,
    align: usize,
    _: &c_void,
) -> *mut c_void {
    cpp_new_impl(size, align, true)
}

/// `operator new[](unsigned long, std::align_val_t)`
#[no_mangle]
pub unsafe extern "C" fn _ZnamSt11align_val_t(size: usize, align: usize) -> *mut c_void {
    cpp_new_impl(size, align, false)
}

/// `operator new(unsigned long, std::align_val_t, std::nothrow_t const&)`
#[no_mangle]
pub unsafe extern "C" fn _ZnwmSt11align_val_tRKSt9nothrow_t(
    size: usize,
    align: usize,
    _: &c_void,
) -> *mut c_void {
    cpp_new_impl(size, align, true)
}

/// `operator new(unsigned long, std::align_val_t)`
#[no_mangle]
pub unsafe extern "C" fn _ZnwmSt11align_val_t(size: usize, align: usize) -> *mut c_void {
    cpp_new_impl(size, align, false)
}

/// `operator delete[](void*, unsigned long)`
#[no_mangle]
pub unsafe extern "C" fn _ZdaPvm(p: *mut c_void, _: usize) {
    free(p);
}

/// `operator delete(void*, unsigned long)`
#[no_mangle]
pub unsafe extern "C" fn _ZdlPvm(p: *mut c_void, _: usize) {
    free(p);
}

/// `operator delete[](void*, std::nothrow_t const&)`
#[no_mangle]
pub unsafe extern "C" fn _ZdaPvRKSt9nothrow_t(p: *mut c_void, _: &c_void) {
    free(p);
}

/// `operator delete(void*, std::nothrow_t const&)`
#[no_mangle]
pub unsafe extern "C" fn _ZdlPvRKSt9nothrow_t(p: *mut c_void, _: &c_void) {
    free(p);
}

/// `operator delete[](void*)`
#[no_mangle]
pub unsafe extern "C" fn _ZdaPv(p: *mut c_void) {
    free(p);
}

/// `operator delete(void*)`
#[no_mangle]
pub unsafe extern "C" fn _ZdlPv(p: *mut c_void) {
    free(p);
}

/// `operator new[](unsigned long, std::nothrow_t const&)`
#[no_mangle]
pub unsafe extern "C" fn _ZnamRKSt9nothrow_t(size: usize, _: &c_void) -> *mut c_void {
    cpp_new_impl(size, 0, true)
}

/// `operator new(unsigned long, std::nothrow_t const&)`
#[no_mangle]
pub unsafe extern "C" fn _ZnwmRKSt9nothrow_t(size: usize, _: &c_void) -> *mut c_void {
    cpp_new_impl(size, 0, true)
}

/// `operator new[](unsigned long)`
#[no_mangle]
pub unsafe extern "C" fn _Znam(size: usize) -> *mut c_void {
    cpp_new_impl(size, 0, false)
}

/// `operator new(unsigned long)`
#[no_mangle]
pub unsafe extern "C" fn _Znwm(size: usize) -> *mut c_void {
    cpp_new_impl(size, 0, false)
}

#[inline]
fn cpp_new_impl(size: usize, align: usize, is_noexcept: bool) -> *mut c_void {
    let ptr = unsafe {
        if align == 0 {
            malloc(size)
        } else {
            aligned_alloc(align, size)
        }
    };
    if !is_noexcept && ptr.is_null() {
        // TODO: Throw an actual C++ exception through `extern "C-unwind"`
        // (rust-lang/rust#74990)
        panic!("allocation of size {} and alignment {} failed", size, align);
    }
    ptr
}
