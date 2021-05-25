//! Free block list mapper. The core implementation of `Tlsf::map_floor`, etc.
use super::{GRANULARITY, GRANULARITY_LOG2, USIZE_BITS};

#[derive(Copy, Clone)]
pub(super) struct MapParams {
    /// `SLLEN.log2()`
    pub sli: u32,
    pub fllen: usize,
}

impl MapParams {
    #[inline]
    fn sllen(&self) -> usize {
        1 << self.sli
    }

    /// Find the free block list to store a free block of the specified size.
    #[inline]
    pub fn map_floor(&self, size: usize) -> Option<(usize, usize)> {
        debug_assert!(size >= GRANULARITY);
        debug_assert!(size % GRANULARITY == 0);
        let fl = USIZE_BITS - GRANULARITY_LOG2 - 1 - size.leading_zeros();

        let sl = if GRANULARITY_LOG2 < self.sli && fl < self.sli - GRANULARITY_LOG2 {
            size << ((self.sli - GRANULARITY_LOG2) - fl)
        } else {
            let sl = size >> (fl + GRANULARITY_LOG2 - self.sli);

            // The most significant one of `size` should be at `sl[SLI]`
            debug_assert!((sl >> self.sli) == 1);

            sl
        };

        // `fl` must be in a valid range
        if fl as usize >= self.fllen {
            return None;
        }

        Some((fl as usize, sl & (self.sllen() - 1)))
    }

    /// Find the first free block list whose every item is at least as large
    /// as the specified size.
    #[inline]
    pub fn map_ceil(&self, size: usize) -> Option<(usize, usize)> {
        debug_assert!(size >= GRANULARITY);
        debug_assert!(size % GRANULARITY == 0);
        let mut fl = USIZE_BITS - GRANULARITY_LOG2 - 1 - size.leading_zeros();

        let sl = if GRANULARITY_LOG2 < self.sli && fl < self.sli - GRANULARITY_LOG2 {
            size << ((self.sli - GRANULARITY_LOG2) - fl)
        } else {
            let mut sl = size >> (fl + GRANULARITY_LOG2 - self.sli);

            // round up (this is specific to `map_ceil`)
            sl += (sl << (fl + GRANULARITY_LOG2 - self.sli) != size) as usize;

            debug_assert!((sl >> self.sli) == 0b01 || (sl >> self.sli) == 0b10);

            // if sl[SLI + 1] { fl += 1; sl = 0; }
            fl += (sl >> (self.sli + 1)) as u32;

            sl
        };

        // `fl` must be in a valid range
        if fl as usize >= self.fllen {
            return None;
        }

        Some((fl as usize, sl & (self.sllen() - 1)))
    }

    /// Find the first free block list whose every item is at least as large
    /// as the specified size and get the list's minimum size. Returns `None`
    /// if there isn't such a list, or the list's minimum size is not
    /// representable in `usize`.
    #[inline]
    pub fn map_ceil_and_unmap(&self, size: usize) -> Option<usize> {
        debug_assert!(size >= GRANULARITY);
        debug_assert!(size % GRANULARITY == 0);

        let map_map_ceil_and_unmap_input = {
            // The maximum value for which `map_ceil(x)` returns `(USIZE_BITS -
            // GRANULARITY_LOG2 - 1, _)`, assuming `FLLEN == ∞`
            let max1 = !(usize::MAX >> (self.sli + 1));

            // Now take into account the fact that `FLLEN` is not actually infinity
            if self.fllen as u32 - 1 < USIZE_BITS - GRANULARITY_LOG2 - 1 {
                max1 >> ((USIZE_BITS - GRANULARITY_LOG2 - 1) - (self.fllen as u32 - 1))
            } else {
                max1
            }
        };

        if size > map_map_ceil_and_unmap_input {
            return None;
        }

        let fl = USIZE_BITS - GRANULARITY_LOG2 - 1 - size.leading_zeros();

        let list_min_size = if GRANULARITY_LOG2 < self.sli && fl < self.sli - GRANULARITY_LOG2 {
            size
        } else {
            let shift = fl + GRANULARITY_LOG2 - self.sli;

            // round up
            (size + ((1 << shift) - 1)) & !((1 << shift) - 1)
        };

        Some(list_min_size)
    }
}
