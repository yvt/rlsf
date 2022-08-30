//! Overrides C memory allocation functions with [`::rlsf`].
use rlsf::CAlloc;
use std::{
    alloc::Layout,
    os::raw::{c_int, c_void},
    ptr::{null_mut, NonNull},
};

#[global_allocator]
pub static ALLOC: rlsf::GlobalTlsf = rlsf::GlobalTlsf::INIT;

// Preserve the symbols
#[used]
static _F: (
    unsafe extern "C" fn(usize) -> *mut c_void,
    unsafe extern "C" fn(*mut c_void) -> usize,
    unsafe extern "C" fn(usize) -> *mut c_void,
    unsafe extern "C" fn(usize) -> *mut c_void,
    unsafe extern "C" fn(usize, usize) -> *mut c_void,
    unsafe extern "C" fn(*mut *mut c_void, usize, usize) -> c_int,
    unsafe extern "C" fn(usize, usize) -> *mut c_void,
    unsafe extern "C" fn(usize, usize) -> *mut c_void,
    unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void,
    unsafe extern "C" fn(*mut c_void),
) = (
    malloc,
    malloc_usable_size,
    valloc,
    pvalloc,
    calloc,
    posix_memalign,
    aligned_alloc,
    memalign,
    realloc,
    free,
);

/// The alignment guaranteed by `malloc`.
const MIN_ALIGN: usize = match () {
    #[cfg(all(any(
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
    )))]
    () => 8,
    #[cfg(all(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "mips64",
        target_arch = "s390x",
        target_arch = "sparc64",
        target_arch = "riscv64"
    )))]
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
