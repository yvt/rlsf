//! The TLSF allocator core
use core::{
    alloc::Layout,
    debug_assert, debug_assert_eq,
    hint::unreachable_unchecked,
    marker::PhantomData,
    mem::{self, MaybeUninit},
    ptr::NonNull,
};

use crate::int::BinInteger;

#[cfg_attr(doc, svgbobdoc::transform)]
/// The TLSF header (top-level) data structure.
///
/// # Data Structure Overview
///
/// <center>
/// ```svgbob
///   First level
///                                                                       FLLEN = 8
///                               ,-----+-----+-----+-----+-----+-----+-----+-----,
///         fl_bitmap: FLBitmap = |  0  |  0  |  0  |  1  |  0  |  0  |  0  |  0  |
///                               +-----+-----+-----+-----+-----+-----+-----+-----+
///                      min size | 2¹¹ | 2¹⁰ |  2⁹ |  2⁸ |  2⁷ |  2⁶ |  2⁵ |  2⁴ |
///                               '-----+-----+-----+--+--+-----+-----+-----+-----'
///                                                    |
/// ╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶|╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶
///   Second Level                                     |
///                                                    v                      SLLEN = 8
///                                  ,-----+-----+-----+-----+-----+-----+-----+-----,
///        "sl_bitmap[4]: SLBitmap"= |  0  |  0  |  1  |  0  |  0  |  0  |  0  |  0  |
///                                  +-----+-----+-----+-----+-----+-----+-----+-----+
///               min size 2⁸(1+n/8) |  7  |  6  |  5  |  4  |  3  |  2  |  1  |  0  |
///                                  +-----+-----+-----+-----+-----+-----+-----+-----+
///                       first_free |     |     |  O  |     |     |     |     |     |
///                                  '-----+-----+--|--+-----+-----+-----+-----+-----'
///                                                 |
///                                                 |  size = 416..448
///                                                 |
/// ╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶|╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶╶
///   Free blocks                                   |
///                                                 |
///             ,-----------------------------------'
///             | ,---+---+-------,    ,---+---+-------,    ,---+---+-------,
///             '-+>O | O-+-------+----+>O | O-+-------+----+>O |   |       |
///               +---+---'       |    +---+---'       |    +---+---'       |
///               |               |    |               |    |               |
///               |               |    |               |    |               |
///               '---------------'    '---------------'    '---------------'
///                   416 bytes            432 bytes            416 bytes
/// ```
/// </center>
///
/// # Properties
///
/// The allocation granularity ([`GRANULARITY`]) is `size_of::<usize>() * 4`
/// bytes, which is the minimum size of a free block.
///
/// The maximum block size is `(GRANULARITY << FLLEN) - GRANULARITY`.
///
#[derive(Debug)]
pub struct Tlsf<'pool, FLBitmap, SLBitmap, const FLLEN: usize, const SLLEN: usize> {
    fl_bitmap: FLBitmap,
    sl_bitmap: [SLBitmap; FLLEN],
    first_free: [[Option<NonNull<FreeBlockHdr>>; SLLEN]; FLLEN],
    _phantom: PhantomData<&'pool mut ()>,
}

// Safety: All memory block headers directly or indirectly referenced by a
//         particular instance of `Tlsf` are logically owned by that `Tlsf` and
//         have no interior mutability, so these are safe.
unsafe impl<FLBitmap, SLBitmap, const FLLEN: usize, const SLLEN: usize> Send
    for Tlsf<'_, FLBitmap, SLBitmap, FLLEN, SLLEN>
{
}

unsafe impl<FLBitmap, SLBitmap, const FLLEN: usize, const SLLEN: usize> Sync
    for Tlsf<'_, FLBitmap, SLBitmap, FLLEN, SLLEN>
{
}

/// The allocation granularity.
///
/// It is `size_of::<usize>() * 4` bytes, which is the minimum size of a TLSF
/// free block.
pub const GRANULARITY: usize = core::mem::size_of::<usize>() * 4;

const GRANULARITY_LOG2: u32 = GRANULARITY.trailing_zeros();

// FIXME: Use `usize::BITS` when it's stable
const USIZE_BITS: u32 = core::mem::size_of::<usize>() as u32 * 8;

/// The header of a memory block.
// The header is actually aligned at `size_of::<usize>() * 4`-byte boundaries
// but the alignment is set to a half value here not to introduce a padding at
// the end of this struct.
#[cfg_attr(target_pointer_width = "16", repr(align(4)))]
#[cfg_attr(target_pointer_width = "32", repr(align(8)))]
#[cfg_attr(target_pointer_width = "64", repr(align(16)))]
#[derive(Debug)]
struct BlockHdr {
    /// The size of the whole memory block, including the header.
    ///
    ///  - `bit[0]` ([`SIZE_USED`]) indicates whether the block is a used memory
    ///    block or not.
    ///
    ///  - `bit[1]` ([`SIZE_LAST_IN_POOL`]) indicates whether the block is the
    ///    last one of the pool or not.
    ///
    ///  - `bit[GRANULARITY_LOG2..]` ([`SIZE_SIZE_MASK`]) represents the size.
    ///
    size: usize,
    prev_phys_block: Option<NonNull<BlockHdr>>,
}

/// The bit of [`BlockHdr::size`] indicating whether the block is a used memory
/// block or not.
const SIZE_USED: usize = 1;
/// The bit of [`BlockHdr::size`] indicating whether the block is the last one
/// of the pool or not.
const SIZE_LAST_IN_POOL: usize = 2;
/// The bits of [`BlockHdr::size`] indicating the block's size.
const SIZE_SIZE_MASK: usize = !((1 << GRANULARITY_LOG2) - 1);

impl BlockHdr {
    /// Get the next block.
    ///
    /// # Safety
    ///
    /// `self.size & SIZE_LAST_IN_POOL` must be telling the truth.
    #[inline]
    unsafe fn next_phys_block(&self) -> Option<NonNull<BlockHdr>> {
        if (self.size & SIZE_LAST_IN_POOL) != 0 {
            None
        } else {
            // Safety: Since `self.size & SIZE_LAST_IN_POOL` is not lying, the
            //         next block should exist at a non-null location.
            Some(
                NonNull::new_unchecked(
                    (self as *const _ as *mut u8).add(self.size & SIZE_SIZE_MASK),
                )
                .cast(),
            )
        }
    }
}

/// The header of a free memory block.
#[repr(C)]
#[cfg_attr(target_pointer_width = "16", repr(align(8)))]
#[cfg_attr(target_pointer_width = "32", repr(align(16)))]
#[cfg_attr(target_pointer_width = "64", repr(align(32)))]
#[derive(Debug)]
struct FreeBlockHdr {
    common: BlockHdr,
    next_free: Option<NonNull<FreeBlockHdr>>,
    prev_free: Option<NonNull<FreeBlockHdr>>,
}

/// The header of a used memory block. It's `GRANULARITY / 2` bytes long.
///
/// The payload immediately follows this header. However, if the alignment
/// requirement is greater than or equal to [`GRANULARITY`], an up to
/// `align - GRANULARITY / 2` bytes long padding will be inserted between them,
/// and the last word of the padding will encode where the header is located.
/// (See `used_block_hdr_for_allocation`)
#[repr(C)]
#[derive(Debug)]
struct UsedBlockHdr {
    common: BlockHdr,
}

impl<FLBitmap: BinInteger, SLBitmap: BinInteger, const FLLEN: usize, const SLLEN: usize> Default
    for Tlsf<'_, FLBitmap, SLBitmap, FLLEN, SLLEN>
{
    fn default() -> Self {
        Self::INIT
    }
}

impl<'pool, FLBitmap: BinInteger, SLBitmap: BinInteger, const FLLEN: usize, const SLLEN: usize>
    Tlsf<'pool, FLBitmap, SLBitmap, FLLEN, SLLEN>
{
    // FIXME: Add `const fn new()` when `const fn`s with type bounds are stabilized
    /// An empty pool.
    pub const INIT: Self = Self {
        fl_bitmap: FLBitmap::ZERO,
        sl_bitmap: [SLBitmap::ZERO; FLLEN],
        first_free: [[None; SLLEN]; FLLEN],
        _phantom: {
            let () = Self::VALID;
            PhantomData
        },
    };

    /// Evaluates successfully if the parameters are valid.
    const VALID: () = {
        if FLLEN == 0 {
            const_panic!("`FLLEN` must not be zero");
        }
        if SLLEN == 0 {
            const_panic!("`SLLEN` must not be zero");
        }
        if (FLBitmap::BITS as u128) < FLLEN as u128 {
            const_panic!("`FLBitmap` should contain at least `FLLEN` bits");
        }
        if (SLBitmap::BITS as u128) < SLLEN as u128 {
            const_panic!("`SLBitmap` should contain at least `SLLEN` bits");
        }
    };

    const MAX_POOL_SIZE: Option<usize> = {
        let shift = GRANULARITY_LOG2 + FLLEN as u32;
        if shift < USIZE_BITS {
            Some((1 << shift) - GRANULARITY)
        } else if shift == USIZE_BITS {
            Some(0usize.wrapping_sub(GRANULARITY))
        } else {
            None
        }
    };

    /// `SLLEN.log2()`
    const SLI: u32 = if SLLEN.is_power_of_two() {
        SLLEN.trailing_zeros()
    } else {
        const_panic!("`SLLEN` is not power of two")
    };

    /// Find the free block list to store a free block of the specified size.
    #[inline]
    fn map_floor(size: usize) -> Option<(usize, usize)> {
        debug_assert!(size >= GRANULARITY);
        debug_assert!(size % GRANULARITY == 0);
        let fl = USIZE_BITS - GRANULARITY_LOG2 - 1 - size.leading_zeros();

        let sl = if GRANULARITY_LOG2 < Self::SLI && fl < Self::SLI - GRANULARITY_LOG2 {
            size << ((Self::SLI - GRANULARITY_LOG2) - fl)
        } else {
            let sl = size >> (fl + GRANULARITY_LOG2 - Self::SLI);

            // The most significant one of `size` should be at `sl[SLI]`
            debug_assert!((sl >> Self::SLI) == 1);

            sl
        };

        // `fl` must be in a valid range
        if fl as usize >= FLLEN {
            return None;
        }

        Some((fl as usize, sl & (SLLEN - 1)))
    }

    /// Find the first free block list whose every item is at least as large
    /// as the specified size.
    #[inline]
    fn map_ceil(size: usize) -> Option<(usize, usize)> {
        debug_assert!(size >= GRANULARITY);
        debug_assert!(size % GRANULARITY == 0);
        let mut fl = USIZE_BITS - GRANULARITY_LOG2 - 1 - size.leading_zeros();

        let sl = if GRANULARITY_LOG2 < Self::SLI && fl < Self::SLI - GRANULARITY_LOG2 {
            size << ((Self::SLI - GRANULARITY_LOG2) - fl)
        } else {
            let mut sl = size >> (fl + GRANULARITY_LOG2 - Self::SLI);

            // round up (this is specific to `map_ceil`)
            sl += (sl << (fl + GRANULARITY_LOG2 - Self::SLI) != size) as usize;

            debug_assert!((sl >> Self::SLI) == 0b01 || (sl >> Self::SLI) == 0b10);

            // if sl[SLI + 1] { fl += 1; sl = 0; }
            fl += (sl >> (Self::SLI + 1)) as u32;

            sl
        };

        // `fl` must be in a valid range
        if fl as usize >= FLLEN {
            return None;
        }

        Some((fl as usize, sl & (SLLEN - 1)))
    }

    /// Insert the specified free block to the corresponding free block list.
    ///
    /// Updates `FreeBlockHdr::{prev_free, next_free}`.
    ///
    /// # Safety
    ///
    ///  - `*block.as_ptr()` must be owned by `self`. (It does not have to be
    ///    initialized, however.)
    ///  - `size` must have a corresponding free list, which does not currently
    ///    contain `block`.
    ///
    unsafe fn link_free_block(&mut self, mut block: NonNull<FreeBlockHdr>, size: usize) {
        let (fl, sl) = Self::map_floor(size).unwrap_or_else(|| unreachable_unchecked());
        let first_free = &mut self.first_free[fl][sl];
        let next_free = mem::replace(first_free, Some(block));
        block.as_mut().next_free = next_free;
        block.as_mut().prev_free = None;

        self.fl_bitmap.set_bit(fl as u32);
        self.sl_bitmap[fl].set_bit(sl as u32);
    }

    /// Remove the specified free block from the corresponding free block list.
    ///
    /// # Safety
    ///
    ///  - `size` must represent the specified free block's size.
    ///  - The free block must be currently included in a free block list.
    ///
    unsafe fn unlink_free_block(&mut self, mut block: NonNull<FreeBlockHdr>, size: usize) {
        let next_free = block.as_mut().next_free;
        let prev_free = block.as_mut().prev_free;

        if let Some(mut next_free) = next_free {
            next_free.as_mut().prev_free = prev_free;
        }

        if let Some(mut prev_free) = prev_free {
            prev_free.as_mut().next_free = next_free;
        } else {
            let (fl, sl) = Self::map_floor(size).unwrap_or_else(|| unreachable_unchecked());
            let first_free = &mut self.first_free[fl][sl];

            debug_assert_eq!(*first_free, Some(block));
            *first_free = next_free;

            if next_free.is_none() {
                // The free list is now empty - update the bitmap
                self.sl_bitmap[fl].clear_bit(sl as u32);
                if self.sl_bitmap[fl] == SLBitmap::ZERO {
                    self.fl_bitmap.clear_bit(fl as u32);
                }
            }
        }
    }

    /// Insert a new free memory block specified by a slice pointer.
    ///
    /// This method does nothing if the given memory block is too small.
    ///
    /// # Time Complexity
    ///
    /// This method will complete in linear time (`O(block.len())`) because
    /// it might need to divide the memory block to meet the maximum block size
    /// requirement (`(GRANULARITY << FLLEN) - GRANULARITY`).
    ///
    /// # Examples
    ///
    /// ```
    /// use minitlsf::Tlsf;
    /// use std::{mem::MaybeUninit, ptr::NonNull};
    /// static mut POOL: MaybeUninit<[u8; 1024]> = MaybeUninit::uninit();
    /// let mut tlsf: Tlsf<u8, u8, 8, 8> = Tlsf::INIT;
    /// unsafe {
    ///     tlsf.insert_free_block_ptr(NonNull::new(POOL.as_mut_ptr()).unwrap());
    /// }
    /// ```
    ///
    /// # Safety
    ///
    /// The memory block will be considered owned by `self`. The memory block
    /// must outlive `self`.
    ///
    /// # Panics
    ///
    /// This method never panics.
    pub unsafe fn insert_free_block_ptr(&mut self, block: NonNull<[u8]>) {
        // FIXME: Use `NonNull<[T]>::len` when it's stable
        //        <https://github.com/rust-lang/rust/issues/71146>
        // Safety: We are just reading the slice length embedded in the fat
        //         pointer and not dereferencing the pointer. We also convert it
        //         to `*mut [MaybeUninit<u8>]` just in case because the slice
        //         might be uninitialized.
        let len = (*(block.as_ptr() as *const [MaybeUninit<u8>])).len();

        // Round up the starting and ending addresses
        let unaligned_start = block.as_ptr() as *mut u8 as usize;
        let start = unaligned_start.wrapping_add(GRANULARITY - 1) & !(GRANULARITY - 1);

        // Calculate the new block length
        let mut size = if let Some(x) = len
            .checked_sub(start.wrapping_sub(unaligned_start))
            .filter(|&x| x >= GRANULARITY)
        {
            // Round down
            x & !(GRANULARITY - 1)
        } else {
            // The block is too small
            return;
        };

        while size > 0 {
            let chunk_size = if let Some(max_pool_size) = Self::MAX_POOL_SIZE {
                size.min(max_pool_size)
            } else {
                size
            };

            debug_assert_eq!(chunk_size % GRANULARITY, 0);

            // The new free block
            // Safety: `start` is not zero.
            let mut block = NonNull::new_unchecked(start as *mut FreeBlockHdr);

            // Initialize the new free block
            block.as_mut().common = BlockHdr {
                size: chunk_size | SIZE_LAST_IN_POOL,
                prev_phys_block: None,
            };

            // Link the free block to the corresponding free list
            self.link_free_block(block, chunk_size);

            size -= chunk_size;
        }
    }

    /// Insert a new free memory block specified by a slice.
    ///
    /// # Time Complexity
    ///
    /// See [`Self::insert_free_block_ptr`].
    ///
    /// # Examples
    ///
    /// ```
    /// use minitlsf::Tlsf;
    /// use std::mem::MaybeUninit;
    /// let mut pool = [MaybeUninit::uninit(); 1024];
    /// let mut tlsf: Tlsf<u8, u8, 8, 8> = Tlsf::INIT;
    /// tlsf.insert_free_block(&mut pool);
    /// ```
    ///
    /// The insertred memory block must outlive `self`:
    ///
    /// ```rust,compile_fail
    /// use minitlsf::Tlsf;
    /// use std::mem::MaybeUninit;
    /// let mut tlsf: Tlsf<u8, u8, 8, 8> = Tlsf::INIT;
    /// let mut pool = [MaybeUninit::uninit(); 1024];
    /// tlsf.insert_free_block(&mut pool);
    /// drop(pool); // dropping the memory block first is not allowed
    /// drop(tlsf);
    /// ```
    ///
    /// # Panics
    ///
    /// This method never panics.
    #[inline]
    pub fn insert_free_block(&mut self, block: &'pool mut [MaybeUninit<u8>]) {
        // Safety: `block` is a mutable reference, which guarantees the absence
        // of aliasing references. Being `'pool` means it will outlive `self`.
        unsafe { self.insert_free_block_ptr(NonNull::new(block as *mut [_] as _).unwrap()) };
    }

    /// Attempt to allocate a block of memory.
    ///
    /// Returns the starting address of the allocated memory block on success;
    /// `None` otherwise.
    ///
    /// # Time Complexity
    ///
    /// This method will complete in constant time.
    pub fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        // Safety: `layout.size()` is already rounded up to `GRANULARITY` bytes
        unsafe { self.allocate_initializing_by(layout, |_| {}) }
    }

    /// Similar to [`Self::allocate`] but `initer` will be called to initialize
    /// the newly allocated memory block before any newly-created block headers
    /// are written.
    #[inline]
    unsafe fn allocate_initializing_by(
        &mut self,
        layout: Layout,
        initer: impl FnOnce(NonNull<u8>),
    ) -> Option<NonNull<u8>> {
        // The extra bytes consumed by the header and padding.
        //
        // After choosing a free block, we need to adjust the payload's location
        // to meet the alignment requirement. Every block is aligned to
        // `GRANULARITY` bytes. `size_of::<UsedBlockHdr>` is `GRANULARITY / 2`
        // bytes, so the address immediately following `UsedBlockHdr` is only
        // aligned to `GRANULARITY / 2` bytes. Consequently, we need to insert
        // a padding containing at most `max(align - GRANULARITY / 2, 0)` bytes.
        let max_overhead =
            layout.align().saturating_sub(GRANULARITY / 2) + mem::size_of::<UsedBlockHdr>();

        // Search for a suitable free block
        let search_size = layout.size().checked_add(max_overhead)?;
        let search_size = search_size.checked_add(GRANULARITY - 1)? & !(GRANULARITY - 1);
        let (fl, sl) = self.search_suitable_free_block_list_for_allocation(search_size)?;

        // Get a free block
        let first_free = &mut self.first_free[fl][sl];
        let block = first_free.unwrap_or_else(|| unreachable_unchecked());
        let next_phys_block = block.as_ref().common.next_phys_block();
        let size_and_flags = block.as_ref().common.size;

        debug_assert!((block.as_ref().common.size & SIZE_USED) == 0);
        debug_assert!((block.as_ref().common.size & SIZE_SIZE_MASK) >= search_size);

        // Unlink the free block. We are not using `unlink_free_block` because
        // we already know `(fl, sl)` and that `block.prev_free` is `None`.
        if let Some(mut next_free) = block.as_ref().next_free {
            next_free.as_mut().prev_free = None;
            *first_free = Some(next_free);
        } else {
            // The free list is now empty - update the bitmap
            self.sl_bitmap[fl].clear_bit(sl as u32);
            if self.sl_bitmap[fl] == SLBitmap::ZERO {
                self.fl_bitmap.clear_bit(fl as u32);
            }
        }

        // Decide the starting address of the payload
        let unaligned_ptr = block.as_ptr() as *mut u8 as usize + mem::size_of::<UsedBlockHdr>();
        let ptr = NonNull::new_unchecked(
            (unaligned_ptr.wrapping_add(layout.align() - 1) & !(layout.align() - 1)) as *mut u8,
        );

        if layout.align() < GRANULARITY {
            debug_assert_eq!(unaligned_ptr, ptr.as_ptr() as usize);
        } else {
            debug_assert_ne!(unaligned_ptr, ptr.as_ptr() as usize);
        }

        // Initialize the payload using a supplied function.
        // This *must* be done before initializing any newly-introduced block
        // headers.
        initer(ptr);

        // Place a header pointer (used by `used_block_hdr_for_allocation`)
        if layout.align() >= GRANULARITY {
            *ptr.cast::<NonNull<_>>().as_ptr().sub(1) = block;
        }

        // Calculate the actual overhead and block size
        let overhead = ptr.as_ptr() as usize - block.as_ptr() as usize;
        debug_assert!(overhead <= max_overhead);

        let new_size = overhead + layout.size();
        let new_size = (new_size + GRANULARITY - 1) & !(GRANULARITY - 1);
        let new_size_and_flags;
        debug_assert!(new_size <= search_size);

        if new_size == size_and_flags & !SIZE_LAST_IN_POOL {
            // The allocation completely fills this free block.
            // Copy `SIZE_LAST_IN_POOL` if `size_and_flags` has one.
            // Updating `next_phys_block.prev_phys_block` is unnecessary in this
            // case because it's still `block`.
            new_size_and_flags = size_and_flags;
        } else {
            // The allocation partially fills this free block. Create a new
            // free block header at `block + new_size_and_flags..block +
            // size_and_flags & SIZE_SIZE_MASK`. The new free block inherits
            // `SIZE_LAST_IN_POOL` from `size_and_flags`.
            let mut new_free_block: NonNull<FreeBlockHdr> =
                NonNull::new_unchecked(block.cast::<u8>().as_ptr().add(new_size)).cast();
            let new_free_block_size_and_flags = size_and_flags - new_size;

            // Update `next_phys_block.prev_phys_block` to point to the new
            // free block
            if let Some(mut next_phys_block) = next_phys_block {
                // Invariant: No two adjacent free blocks
                debug_assert!((next_phys_block.as_ref().size & SIZE_USED) != 0);

                next_phys_block.as_mut().prev_phys_block = Some(new_free_block.cast());
            }

            debug_assert!((new_free_block_size_and_flags & SIZE_USED) == 0);
            new_free_block.as_mut().common = BlockHdr {
                size: new_free_block_size_and_flags,
                prev_phys_block: Some(block.cast()),
            };
            self.link_free_block(
                new_free_block,
                new_free_block_size_and_flags & SIZE_SIZE_MASK,
            );

            // The new used block won't have `SIZE_LAST_IN_POOL`
            new_size_and_flags = new_size;
        }

        // Turn `block` into a used memory block and initialize the used block
        // header. `prev_phys_block` is already set.
        let mut block = block.cast::<UsedBlockHdr>();
        block.as_mut().common.size = new_size_and_flags | SIZE_USED;

        Some(ptr)
    }

    /// Search for a non-empty free block list for allocation.
    #[inline]
    fn search_suitable_free_block_list_for_allocation(
        &self,
        min_size: usize,
    ) -> Option<(usize, usize)> {
        let (mut fl, mut sl) = Self::map_ceil(min_size)?;

        // Search in range `(fl, sl..SLLEN)`
        sl = self.sl_bitmap[fl].bit_scan_forward(sl as u32) as usize;
        if sl < SLLEN {
            debug_assert!(self.sl_bitmap[fl].get_bit(sl as u32));

            return Some((fl, sl));
        }

        // Search in range `(fl + 1.., ..)`
        fl = self.fl_bitmap.bit_scan_forward(fl as u32 + 1) as usize;
        if fl < FLLEN {
            debug_assert!(self.fl_bitmap.get_bit(fl as u32));

            sl = self.sl_bitmap[fl].trailing_zeros() as usize;
            if sl >= SLLEN {
                debug_assert!(false);
                unsafe { unreachable_unchecked() };
            }

            debug_assert!(self.sl_bitmap[fl].get_bit(sl as u32));
            Some((fl, sl))
        } else {
            None
        }
    }

    /// Find the `UsedBlockHdr` for an allocation (any `NonNull<u8>` returned by
    /// our allocation functions).
    ///
    /// # Safety
    ///
    ///  - `ptr` must point to an allocated memory block returned by
    ///      `Self::{allocate, reallocate}`.
    ///
    ///  - The memory block must have been allocated with the same alignment
    ///    ([`Layout::align`]) as `align`.
    ///
    #[inline]
    unsafe fn used_block_hdr_for_allocation(
        ptr: NonNull<u8>,
        align: usize,
    ) -> NonNull<UsedBlockHdr> {
        if align >= GRANULARITY {
            // Read the header pointer at `ptr - USIZE_LEN`
            *ptr.cast::<NonNull<UsedBlockHdr>>().as_ptr().sub(1)
        } else {
            NonNull::new_unchecked(ptr.as_ptr().sub(GRANULARITY / 2)).cast()
        }
    }

    /// Deallocate a previously allocated memory block.
    ///
    /// # Time Complexity
    ///
    /// This method will complete in constant time.
    ///
    /// # Safety
    ///
    ///  - `ptr` must denote a memory block previously allocated via `self`.
    ///  - The memory block must have been allocated with the same alignment
    ///    ([`Layout::align`]) as `align`.
    ///
    pub unsafe fn deallocate(&mut self, ptr: NonNull<u8>, align: usize) {
        // Safety: `ptr` is a previously allocated memory block with the same
        //         alignment as `align`. This is upheld by the caller.
        let mut block = Self::used_block_hdr_for_allocation(ptr, align).cast::<BlockHdr>();
        let mut size = block.as_ref().size & !SIZE_USED;
        debug_assert!((block.as_ref().size & SIZE_USED) != 0);

        // This variable tracks whose `prev_phys_block` we should update.
        let new_next_phys_block;

        // Merge with the next block if it's a free block
        // Safety: `block.common` should be fully up-to-date and valid
        if let Some(next_phys_block) = block.as_ref().next_phys_block() {
            debug_assert!((size & SIZE_LAST_IN_POOL) == 0);

            let next_phys_block_size = next_phys_block.as_ref().size;
            if (next_phys_block_size & SIZE_USED) == 0 {
                // It's coalescable. Add its size to `size`. This will transfer
                // any `SIZE_LAST_IN_POOL` flag `next_phys_block` may have at
                // the same time.
                size += next_phys_block_size;

                new_next_phys_block = next_phys_block.as_ref().next_phys_block();

                // Unlink `next_phys_block`.
                self.unlink_free_block(
                    next_phys_block.cast(),
                    next_phys_block_size & SIZE_SIZE_MASK,
                );
            } else {
                new_next_phys_block = Some(next_phys_block);
            }
        } else {
            new_next_phys_block = None;
        }

        // Merge with the previous block if it's a free block.
        if let Some(prev_phys_block) = block.as_ref().prev_phys_block {
            let prev_phys_block_size = prev_phys_block.as_ref().size;
            debug_assert!((prev_phys_block_size & SIZE_LAST_IN_POOL) == 0);

            if (prev_phys_block_size & SIZE_USED) == 0 {
                // It's coalescable. Add its size to `size`.
                size += prev_phys_block_size;

                // Unlink `prev_phys_block`.
                debug_assert_eq!(prev_phys_block_size & SIZE_SIZE_MASK, prev_phys_block_size);
                self.unlink_free_block(prev_phys_block.cast(), prev_phys_block_size);

                // Move `block` to where `prev_phys_block` is located. By doing
                // this, `block` will implicitly inherit `prev_phys_block.
                // as_ref().prev_phys_block`.
                block = prev_phys_block;
            }
        }

        // Write the new free block's size and flags.
        debug_assert!((size & SIZE_USED) == 0);
        block.as_mut().size = size;

        // Link this free block to the corresponding free list
        let block = block.cast::<FreeBlockHdr>();
        self.link_free_block(block, size & !SIZE_LAST_IN_POOL);

        // Link `new_next_phys_block.prev_phys_block` to `block`
        if let Some(mut new_next_phys_block) = new_next_phys_block {
            debug_assert_eq!(
                Some(new_next_phys_block),
                block.as_ref().common.next_phys_block()
            );
            new_next_phys_block.as_mut().prev_phys_block = Some(block.cast());
        }
    }

    /// Shrink or grow a previously allocated memory block.
    ///
    /// Returns the new starting address of the memory block on success;
    /// `None` otherwise.
    ///
    /// # Time Complexity
    ///
    /// Unlike other methods, this method will complete in linear time
    /// (`O(old_size)`).
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
        // Safety: `ptr` is a previously allocated memory block with the same
        //         alignment as `align`. This is upheld by the caller.
        let block = Self::used_block_hdr_for_allocation(ptr, new_layout.align());

        // Round up the allocation size. Fail if this causes an overflow.
        let new_layout = Layout::from_size_align_unchecked(
            new_layout.size().checked_add(GRANULARITY - 1)? & !(GRANULARITY - 1),
            new_layout.align(),
        );

        // First try to shrink or grow the block toward the end (i.e., preseving
        // the starting address).
        if let Some(x) = self.reallocate_without_moving(ptr, block, new_layout) {
            return Some(x);
        }

        // Preserve the first `GRANULARITY / 2` bytes
        let mut head: MaybeUninit<[usize; 2]> = MaybeUninit::uninit();
        core::ptr::copy_nonoverlapping(ptr.as_ptr(), head.as_mut_ptr() as *mut u8, GRANULARITY / 2);

        // Deallocate the old memory block. This will not invalidate the
        // contained data except for the first `GRANULARITY / 2` bytes (because
        // `FreeBlockHdr` is larger than `UsedBlockHdr` by `GRANULARITY / 2`
        // bytes).
        self.deallocate(ptr, new_layout.align());

        // Allocate a whole new memory block
        self.allocate_initializing_by(
            new_layout,
            #[inline]
            |new_alloc| {
                // The contained data is still intact at this point except for
                // the first `GRANULARITY / 2` bytes. Move it into the new
                // location
                core::ptr::copy(
                    ptr.as_ptr().add(mem::size_of::<[usize; 2]>()),
                    new_alloc.as_ptr().add(mem::size_of::<[usize; 2]>()),
                    (block.as_ref().common.size & SIZE_SIZE_MASK)
                        - mem::size_of::<UsedBlockHdr>()
                        - mem::size_of::<[usize; 2]>(),
                );

                // The first `GRANULARITY / 2` bytes might have been overwritten
                // by a new `FreeBlockHdr`, so it must be copied from `head`
                core::ptr::copy(
                    head.as_ptr() as *const u8,
                    new_alloc.as_ptr(),
                    mem::size_of::<[usize; 2]>(),
                );
            },
        )
    }

    /// A subroutine of [`Self::reallocate`]. Attempts to shrink or grow the
    // block toward the end (i.e., preseving the starting address).
    #[inline]
    unsafe fn reallocate_without_moving(
        &mut self,
        ptr: NonNull<u8>,
        mut block: NonNull<UsedBlockHdr>,
        new_layout: Layout,
    ) -> Option<NonNull<u8>> {
        // The extra bytes consumed by the header and any padding
        let overhead = ptr.as_ptr() as usize - block.as_ptr() as usize;

        // Calculate the new block size. Fail if this causes an overflow.
        // Failing at this point does not necessarily mean the whole process of
        // reallocation has failed; a new place with a smaller overhead could be
        // found later (whether there's actually such a situation or not is yet
        // to be proven).
        let mut new_size = overhead.checked_add(new_layout.size())?;

        let old_size = block.as_ref().common.size & SIZE_SIZE_MASK;

        if new_size > old_size {
            let grow_by = new_size - old_size;

            // Grow into the next free block. Fail if there isn't such a block.
            let next_phys_block = block.as_ref().common.next_phys_block()?;
            let mut next_phys_block_size_and_flags = next_phys_block.as_ref().size;
            let mut next_phys_block_size = next_phys_block_size_and_flags & SIZE_SIZE_MASK;

            // Fail it isn't a free block.
            if (next_phys_block_size_and_flags & SIZE_USED) != 0 {
                return None;
            }

            // Now we know it's really a free block.
            let mut next_phys_block = next_phys_block.cast::<FreeBlockHdr>();
            let next_next_phys_block = next_phys_block.as_ref().common.next_phys_block();

            if grow_by > next_phys_block_size {
                // Can't fit
                return None;
            }

            self.unlink_free_block(next_phys_block, next_phys_block_size);

            if grow_by < next_phys_block_size {
                // Can fit and there's some slack. Create a free block, which
                // will inherit the original free block's `SIZE_LAST_IN_POOL`.
                next_phys_block_size_and_flags -= grow_by;
                next_phys_block_size -= grow_by;

                next_phys_block =
                    NonNull::new_unchecked(block.cast::<u8>().as_ptr().add(new_size)).cast();
                debug_assert!((next_phys_block_size_and_flags & SIZE_USED) == 0);
                next_phys_block.as_mut().common = BlockHdr {
                    size: next_phys_block_size_and_flags,
                    prev_phys_block: Some(block.cast()),
                };
                self.link_free_block(next_phys_block, next_phys_block_size);

                // Update `next_next_phys_block.prev_phys_block` if necessary
                if let Some(mut next_next_phys_block) = next_next_phys_block {
                    next_next_phys_block.as_mut().prev_phys_block = Some(next_phys_block.cast());
                }
            } else {
                // Can fit exactly. Copy the `SIZE_LAST_IN_POOL` flag if
                // `next_phys_block` has one.
                new_size += next_phys_block_size_and_flags & SIZE_LAST_IN_POOL;

                // Update `next_next_phys_block.prev_phys_block` if necessary
                if let Some(mut next_next_phys_block) = next_next_phys_block {
                    next_next_phys_block.as_mut().prev_phys_block = Some(block.cast());
                }
            }

            block.as_mut().common.size = new_size | SIZE_USED;
        } else if new_size < old_size {
            // Shrink the block, creating a new free block at the end
            let shrink_by = new_size - old_size;

            // We will create a new free block at this address
            let mut new_free_block: NonNull<FreeBlockHdr> =
                NonNull::new_unchecked(block.cast::<u8>().as_ptr().add(new_size)).cast();
            let mut new_free_block_size_and_flags =
                shrink_by + (block.as_ref().common.size & SIZE_LAST_IN_POOL);

            // If the next block is a free block...
            if let Some(mut next_phys_block) = block.as_ref().common.next_phys_block() {
                let next_phys_block_size_and_flags = next_phys_block.as_ref().size;
                let next_phys_block_size = next_phys_block_size_and_flags & SIZE_SIZE_MASK;

                if (next_phys_block_size_and_flags & SIZE_USED) == 0 {
                    // Then we can merge this existing free block (`next_phys_block`)
                    // into the new one (`new_free_block`). Copy `SIZE_LAST_IN_POOL`
                    // as well if `next_phys_block` has one.
                    self.unlink_free_block(next_phys_block.cast(), next_phys_block_size);
                    new_free_block_size_and_flags += next_phys_block_size;

                    if let Some(mut next_next_phys_block) =
                        next_phys_block.as_ref().next_phys_block()
                    {
                        next_next_phys_block.as_mut().prev_phys_block = Some(new_free_block.cast());
                    }
                } else {
                    // We can't merge an used block (`next_phys_block`) and
                    // a free block (`new_free_block`).
                    next_phys_block.as_mut().prev_phys_block = Some(new_free_block.cast());
                }
            }

            debug_assert!((new_free_block_size_and_flags & SIZE_USED) == 0);
            new_free_block.as_mut().common = BlockHdr {
                size: new_free_block_size_and_flags,
                prev_phys_block: Some(block.cast()),
            };
            self.link_free_block(
                new_free_block,
                new_free_block_size_and_flags & SIZE_SIZE_MASK,
            );

            block.as_mut().common.size = new_size | SIZE_USED;
        } else {
            // No size change
        }

        Some(ptr)
    }
}

#[cfg(test)]
mod tests;
