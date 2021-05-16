//! An allocator with flexible backing stores
use core::{alloc::Layout, debug_assert, ptr::NonNull, unimplemented};

use super::{int::BinInteger, Init, Tlsf, GRANULARITY};

/// The trait for dynamic storage allocators that can back [`FlexTlsf`].
pub unsafe trait FlexSource {
    /// Allocate a memory block of the requested minimum size.
    ///
    /// Returns the address range of the allocated memory block.
    ///
    /// # Safety
    ///
    /// `min_size` must be a multiple of [`GRANULARITY`]. `min_size` must not
    /// be zero.
    #[inline]
    unsafe fn alloc(&mut self, min_size: usize) -> Option<[NonNull<u8>; 2]> {
        let _ = min_size;
        None
    }

    /// Attempt to grow the specified allocation without moving it. Returns
    /// the memory allocation's end address on success.
    ///
    /// # Safety
    ///
    /// `[start, old_end]` must be an existing allocation made by this
    /// allocator. `min_new_end` must be greater than or equal to `old_end`.
    #[inline]
    unsafe fn realloc_inplace_grow(
        &mut self,
        start: NonNull<u8>,
        old_end: NonNull<u8>,
        min_new_end: NonNull<u8>,
    ) -> Option<NonNull<u8>> {
        let _ = (start, old_end, min_new_end);
        None
    }

    /// Deallocate a previously allocated memory block.
    ///
    /// # Safety
    ///
    /// `[start, end]` must denote an existing allocation made by this
    /// allocator.
    #[inline]
    unsafe fn dealloc(&mut self, [start, end]: [NonNull<u8>; 2]) {
        let _ = [start, end];
        unimplemented!("`supports_dealloc` returned `true`, but `dealloc` is not implemented");
    }

    /// Check if this allocator implements [`Self::dealloc`].
    ///
    /// If this method returns `false`, [`FlexTlsf`] will not call `dealloc` to
    /// release memory blocks. It also applies some optimizations.
    ///
    /// The returned value must be constant for a particular instance of `Self`.
    #[inline]
    fn supports_dealloc(&self) -> bool {
        false
    }

    /// Get the minimum alignment of allocations made by this allocator.
    /// [`FlexTlsf`] may be less efficient if this method returns a value
    /// less than [`GRANULARITY`].
    #[inline]
    fn min_align(&self) -> usize {
        1
    }
}

/// Wraps [`core::alloc::GlobalAlloc`] to implement the [`FlexSource`] trait.
///
/// Since this type does not implement [`FlexSource::realloc_inplace_grow`],
/// it is likely to end up with terribly fragmented memory pools.
#[derive(Default, Debug, Copy, Clone)]
pub struct GlobalAllocAsFlexSource<T, const ALIGN: usize>(pub T);

impl<T: core::alloc::GlobalAlloc, const ALIGN: usize> GlobalAllocAsFlexSource<T, ALIGN> {
    const ALIGN: usize = if ALIGN.is_power_of_two() {
        if ALIGN < GRANULARITY {
            GRANULARITY
        } else {
            ALIGN
        }
    } else {
        const_panic!("`ALIGN` is not power of two")
    };
}

impl<T: Init, const ALIGN: usize> Init for GlobalAllocAsFlexSource<T, ALIGN> {
    const INIT: Self = Self(Init::INIT);
}

unsafe impl<T: core::alloc::GlobalAlloc, const ALIGN: usize> FlexSource
    for GlobalAllocAsFlexSource<T, ALIGN>
{
    #[inline]
    unsafe fn alloc(&mut self, min_size: usize) -> Option<[NonNull<u8>; 2]> {
        let layout = Layout::from_size_align(min_size, Self::ALIGN)
            .ok()?
            .pad_to_align();
        // Safety: The caller upholds that `min_size` is not zero
        let start = self.0.alloc(layout);
        let start = NonNull::new(start)?;
        let end = if let Some(x) = NonNull::new(start.as_ptr().wrapping_add(layout.size())) {
            x
        } else {
            unimplemented!()
        };
        Some([start, end])
    }

    #[inline]
    unsafe fn dealloc(&mut self, [start, end]: [NonNull<u8>; 2]) {
        // Safety: This layout was previously used for allocation, during which
        //         the layout was checked for validity
        let layout = Layout::from_size_align_unchecked(
            end.as_ptr() as usize - start.as_ptr() as usize,
            Self::ALIGN,
        );

        // Safety: `start` denotes an existing allocation with layout `layout`
        self.0.dealloc(start.as_ptr(), layout);
    }

    fn supports_dealloc(&self) -> bool {
        true
    }

    #[inline]
    fn min_align(&self) -> usize {
        Self::ALIGN
    }
}

/// A wrapper of [`Tlsf`] that automatically acquires fresh memory pools from
/// [`FlexSource`].
#[derive(Debug)]
pub struct FlexTlsf<Source: FlexSource, FLBitmap, SLBitmap, const FLLEN: usize, const SLLEN: usize>
{
    source: Source,
    tlsf: Tlsf<'static, FLBitmap, SLBitmap, FLLEN, SLLEN>,
    /// The lastly created memory pool.
    growable_pool: Option<Pool>,
}

#[derive(Debug, Copy, Clone)]
struct Pool {
    /// The starting address of the memory allocation.
    alloc_start: NonNull<u8>,
    /// The ending address of the memory allocation.
    alloc_end: NonNull<u8>,
    /// The ending address of the memory pool created within the allocation.
    /// This might be slightly less than `alloc_end`.
    pool_end: NonNull<u8>,
}

// Safety: `Pool` is totally thread-safe
unsafe impl Send for Pool {}
unsafe impl Sync for Pool {}

/// Pool footer stored at the end of each pool. It's only used when
/// supports_dealloc() == true`.
///
/// The footer is stored in the sentinel block's unused space or any padding
/// present at the end of each pool. This is why `PoolFtr` can't be larger than
/// two `usize`s.
#[repr(C)]
#[derive(Copy, Clone)]
struct PoolFtr {
    /// The previous allocation's end address. Forms a singly-linked list.
    prev_alloc_end: Option<NonNull<u8>>,
    /// This allocation's start address. It's uninitialized while the allocation
    /// is in `FlexTlsf::growable_pool`.
    alloc_start: NonNull<u8>,
}

const _: () = if core::mem::size_of::<PoolFtr>() != GRANULARITY / 2 {
    const_panic!("bad `PoolFtr` size");
};

impl PoolFtr {
    /// Get a pointer to `PoolFtr` for a given allocation.
    #[inline]
    fn get_for_alloc_end(alloc_end: NonNull<u8>, alloc_align: usize) -> *mut Self {
        let mut ptr = alloc_end
            .as_ptr()
            .wrapping_sub(core::mem::size_of::<Self>());
        // If `alloc_end` is not well-aligned, we need to adjust the location
        // of `PoolFtr`
        if alloc_align < core::mem::align_of::<Self>() {
            ptr = (ptr as usize & !(core::mem::align_of::<Self>() - 1)) as _;
        }
        ptr as _
    }
}

/// Initialization with a [`FlexSource`] provided by [`Default::default`]
impl<
        Source: FlexSource + Default,
        FLBitmap: BinInteger,
        SLBitmap: BinInteger,
        const FLLEN: usize,
        const SLLEN: usize,
    > Default for FlexTlsf<Source, FLBitmap, SLBitmap, FLLEN, SLLEN>
{
    #[inline]
    fn default() -> Self {
        Self {
            source: Source::default(),
            tlsf: Tlsf::INIT,
            growable_pool: None,
        }
    }
}

/// Initialization with a [`FlexSource`] provided by [`Init::INIT`]
impl<
        Source: FlexSource + Init,
        FLBitmap: BinInteger,
        SLBitmap: BinInteger,
        const FLLEN: usize,
        const SLLEN: usize,
    > Init for FlexTlsf<Source, FLBitmap, SLBitmap, FLLEN, SLLEN>
{
    // FIXME: Add `const fn new()` when `const fn`s with type bounds are stabilized
    /// An empty pool.
    const INIT: Self = Self {
        source: Source::INIT,
        tlsf: Tlsf::INIT,
        growable_pool: None,
    };
}

impl<
        Source: FlexSource,
        FLBitmap: BinInteger,
        SLBitmap: BinInteger,
        const FLLEN: usize,
        const SLLEN: usize,
    > FlexTlsf<Source, FLBitmap, SLBitmap, FLLEN, SLLEN>
{
    /// Construct a new `FlexTlsf` object.
    #[inline]
    pub fn new(source: Source) -> Self {
        Self {
            source,
            tlsf: Tlsf::INIT,
            growable_pool: None,
        }
    }

    /// Borrow the contained `Source`.
    #[inline]
    pub fn source_ref(&self) -> &Source {
        &self.source
    }

    /// Mutably borrow the contained `Source`.
    ///
    /// # Safety
    ///
    /// The caller must not replace the `Source` with another one or modify
    /// any existing allocations in the `Source`.
    #[inline]
    pub unsafe fn source_mut_unchecked(&mut self) -> &mut Source {
        &mut self.source
    }

    /// Attempt to allocate a block of memory.
    ///
    /// Returns the starting address of the allocated memory block on success;
    /// `None` otherwise.
    ///
    /// # Time Complexity
    ///
    /// This method will complete in constant time (assuming `Source`'s methods
    /// do so as well).
    pub fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        if let Some(x) = self.tlsf.allocate(layout) {
            return Some(x);
        }

        self.increase_pool_to_contain_allocation(layout)?;

        self.tlsf.allocate(layout).or_else(|| {
            // Not a hard error, but it's still unexpected because
            // `increase_pool_to_contain_allocation` was supposed to make this
            // allocation possible
            debug_assert!(
                false,
                "the allocation failed despite the effort by \
                `increase_pool_to_contain_allocation`"
            );
            None
        })
    }

    /// Increase the amount of memory pool to guarantee the success of the
    /// given allocation. Returns `Some(())` on success.
    #[inline]
    fn increase_pool_to_contain_allocation(&mut self, layout: Layout) -> Option<()> {
        // How many extra bytes we need to get from the source for the
        // allocation to success?
        let extra_bytes_well_aligned =
            Tlsf::<'static, FLBitmap, SLBitmap, FLLEN, SLLEN>::pool_size_to_contain_allocation(
                layout,
            )?;

        // The sentinel block + the block to store the allocation
        debug_assert!(extra_bytes_well_aligned >= GRANULARITY * 2);

        if let Some(growable_pool) = self.growable_pool {
            // Try to extend an existing memory pool first.
            let new_pool_end_desired = unsafe {
                NonNull::new_unchecked(
                    (growable_pool.pool_end.as_ptr() as usize)
                        .checked_add(extra_bytes_well_aligned)? as *mut u8,
                )
            };

            // The following assertion should not trip because...
            //  - `extra_bytes_well_aligned` returns a value that is at least
            //    as large as `GRANULARITY * 2`.
            //  - `growable_pool.alloc_end - growable_pool.pool_end` must be
            //    less than `GRANULARITY * 2` because of
            //    `insert_free_block_ptr`'s implementation.
            debug_assert!(new_pool_end_desired >= growable_pool.alloc_end);

            // Safety: `new_pool_end_desired >= growable_pool.alloc_end`, and
            //         `[growable_pool.alloc_start, growable_pool.alloc_end]`
            //         represents a previous allocation.
            if let Some(new_alloc_end) = unsafe {
                self.source.realloc_inplace_grow(
                    growable_pool.alloc_start,
                    growable_pool.alloc_end,
                    new_pool_end_desired,
                )
            } {
                if self.source.supports_dealloc() {
                    // Move `PoolFtr`. Note that `PoolFtr::alloc_start` is
                    // still uninitialized because this allocation is still in
                    // `self.growable_pool`, so we only have to move
                    // `PoolFtr::prev_alloc_end`.
                    let old_pool_ftr = PoolFtr::get_for_alloc_end(
                        growable_pool.alloc_end,
                        self.source.min_align(),
                    );
                    let new_pool_ftr =
                        PoolFtr::get_for_alloc_end(new_alloc_end, self.source.min_align());
                    // Safety: Both `(*new_pool_ftr).prev_alloc_end` and
                    //         `(*old_pool_ftr).prev_alloc_end` are within
                    //         pool footers we control
                    unsafe { (*new_pool_ftr).prev_alloc_end = (*old_pool_ftr).prev_alloc_end };
                }

                // Safety: `growable_pool.pool_end` is the end address of an
                //         existing memory pool, and the passed memory block is
                //         owned by us
                let new_pool_end = unsafe {
                    self.tlsf
                        .append_free_block_ptr(nonnull_slice_from_raw_parts(
                            growable_pool.pool_end,
                            new_alloc_end.as_ptr() as usize
                                - growable_pool.pool_end.as_ptr() as usize,
                        ))
                };

                // This assumption is based on `extra_bytes_well_aligned`'s
                // implementation. The `debug_assert!` above depends on this.
                debug_assert!(
                    (new_alloc_end.as_ptr() as usize - new_pool_end.as_ptr() as usize)
                        < GRANULARITY * 2
                );

                return Some(());
            }
        }

        // Create a brand new allocation. `source.min_align` indicates the
        // minimum alignment that the created allocation will satisfy.
        // `extra_bytes_well_aligned` is the pool size that can contain the
        // allocation *if* the pool was well-aligned. If `source.min_align` is
        // not well-aligned enough, we need to allocate extra bytes.
        let extra_bytes = if self.source.min_align() < GRANULARITY {
            //
            //                    wasted                             wasted
            //                     ╭┴╮                               ╭──┴──╮
            //                     ┌─┬─┬─┬─┬─┬─┬─┬─┬─┬─┬─┬─┬─┬─┬─┬─┬─┬─┬─┬─┐
            //         Allocation: │ │ │ │ │ │ │ │ │ │ │ │ │ │ │ │ │ │ │ │ │
            //                     └─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┘
            //                       ┌───────┬───────┬───────┬───────┐
            // Pool created on it:   │       │       │       │       │
            //                       └───────┴───────┴───────┴───────┘
            //                       ╰───┬───╯
            //                      GRANULARITY
            //
            extra_bytes_well_aligned.checked_add(GRANULARITY)?
        } else {
            extra_bytes_well_aligned
        };

        // Safety: `extra_bytes` is non-zero and aligned to `GRANULARITY` bytes
        let [alloc_start, alloc_end] = unsafe { self.source.alloc(extra_bytes)? };

        // Safety: The passed memory block is what we acquired from
        //         `self.source`, so we have the ownership
        let [_, pool_end] = unsafe {
            self.tlsf
                .insert_free_block_ptr(nonnull_slice_from_raw_parts(
                    alloc_start,
                    alloc_end.as_ptr() as usize - alloc_start.as_ptr() as usize,
                ))
        }
        .unwrap_or_else(|| unsafe {
            debug_assert!(false, "`pool_size_to_contain_allocation` is an impostor");
            // Safety: It's unreachable
            core::hint::unreachable_unchecked()
        });

        if self.source.supports_dealloc() {
            // Link the new memory pool's `PoolFtr::prev_alloc_end` to the
            // previous pool (`self.growable_pool`).
            let pool_ftr = PoolFtr::get_for_alloc_end(alloc_end, self.source.min_align());
            let prev_alloc_end = self.growable_pool.map(|p| p.alloc_end);
            // Safety: `(*pool_ftr).prev_alloc_end` is within a pool footer
            //         we control
            unsafe { (*pool_ftr).prev_alloc_end = prev_alloc_end };

            // Set the previous pool's `PoolFtr::alloc_start`.
            if let Some(p) = self.growable_pool {
                let prev_pool_ftr =
                    PoolFtr::get_for_alloc_end(p.alloc_end, self.source.min_align());
                // Safety: `(*prev_pool_ftr).alloc_start` is within a pool
                // footer we control
                unsafe { (*prev_pool_ftr).alloc_start = p.alloc_start };
            }
        }

        self.growable_pool = Some(Pool {
            alloc_start,
            alloc_end,
            pool_end,
        });

        Some(())
    }

    /// Deallocate a previously allocated memory block.
    ///
    /// # Time Complexity
    ///
    /// This method will complete in constant time (assuming `Source`'s methods
    /// do so as well).
    ///
    /// # Safety
    ///
    ///  - `ptr` must denote a memory block previously allocated via `self`.
    ///  - The memory block must have been allocated with the same alignment
    ///    ([`Layout::align`]) as `align`.
    ///
    #[inline]
    pub unsafe fn deallocate(&mut self, ptr: NonNull<u8>, align: usize) {
        // Safety: Upheld by the caller
        self.tlsf.deallocate(ptr, align)
    }

    /// Shrink or grow a previously allocated memory block.
    ///
    /// Returns the new starting address of the memory block on success;
    /// `None` otherwise.
    ///
    /// # Time Complexity
    ///
    /// Unlike other methods, this method will complete in linear time
    /// (`O(old_size)`), assuming `Source`'s methods do so as well.
    ///
    /// # Safety
    ///
    ///  - `ptr` must denote a memory block previously allocated via `self`.
    ///  - The memory block must have been allocated with the same alignment
    ///    ([`Layout::align`]) as `new_layout`.
    ///
    pub unsafe fn reallocate(
        &mut self,
        ptr: NonNull<u8>,
        new_layout: Layout,
    ) -> Option<NonNull<u8>> {
        // Do this early so that the compiler can de-duplicate the evaluation of
        // `size_of_allocation`, which is done here as well as in
        // `Tlsf::reallocate`.
        let old_size = Tlsf::<'static, FLBitmap, SLBitmap, FLLEN, SLLEN>::size_of_allocation(
            ptr,
            new_layout.align(),
        );

        // Safety: Upheld by the caller
        if let Some(x) = self.tlsf.reallocate(ptr, new_layout) {
            return Some(x);
        }

        // Allocate a whole new memory block. The following code section looks
        // the same as the one in `Tlsf::reallocate`, but `self.allocation`
        // here refers to `FlexTlsf::allocate`, which inserts new meory pools
        // as necessary.
        let new_ptr = self.allocate(new_layout)?;

        // Move the existing data into the new location
        debug_assert!(new_layout.size() >= old_size);
        core::ptr::copy_nonoverlapping(ptr.as_ptr(), new_ptr.as_ptr(), old_size);

        // Deallocate the old memory block.
        self.deallocate(ptr, new_layout.align());

        Some(new_ptr)
    }
}

impl<Source: FlexSource, FLBitmap, SLBitmap, const FLLEN: usize, const SLLEN: usize> Drop
    for FlexTlsf<Source, FLBitmap, SLBitmap, FLLEN, SLLEN>
{
    fn drop(&mut self) {
        if self.source.supports_dealloc() {
            // Deallocate all memory pools
            let align = self.source.min_align();
            let mut cur = self.growable_pool.map(|p| {
                let hdr = PoolFtr::get_for_alloc_end(p.alloc_end, align);
                // Safety: `(*hdr).prev_alloc_end` is within a pool footer we
                //         still control
                (p.alloc_start, p.alloc_end, unsafe { (*hdr).prev_alloc_end })
            });

            while let Some((alloc_start, alloc_end, prev_alloc_end)) = cur {
                // Safety: It's an allocation we allocated from `self.source`
                unsafe { self.source.dealloc([alloc_start, alloc_end]) };

                cur = prev_alloc_end.map(|prev_alloc_end| {
                    // Safety: We control the referenced pool footer
                    let prev_hdr = unsafe { *PoolFtr::get_for_alloc_end(prev_alloc_end, align) };
                    (
                        prev_hdr.alloc_start,
                        prev_alloc_end,
                        prev_hdr.prev_alloc_end,
                    )
                });
            }
        }
    }
}

/// Polyfill for <https://github.com/rust-lang/rust/issues/71941>
#[inline]
fn nonnull_slice_from_raw_parts<T>(ptr: NonNull<T>, len: usize) -> NonNull<[T]> {
    unsafe { NonNull::new_unchecked(core::ptr::slice_from_raw_parts_mut(ptr.as_ptr(), len)) }
}

#[cfg(test)]
mod tests;
