extern crate std;

use std::{alloc::Layout, collections::BTreeMap, ops::Range, ptr::NonNull};

pub struct ShadowAllocator {
    regions: BTreeMap<usize, SaRegion>,
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum SaRegion {
    Free,
    Used,
    Invalid,
}

impl ShadowAllocator {
    pub fn new() -> Self {
        Self {
            regions: Some((0, SaRegion::Invalid)).into_iter().collect(),
        }
    }

    pub fn convert_range(
        &mut self,
        range: Range<usize>,
        old_region: SaRegion,
        new_region: SaRegion,
    ) {
        if range.len() == 0 {
            return;
        }

        assert_ne!(old_region, new_region);
        log::trace!(
            "sa: converting {:?} from {:?} to {:?}",
            range,
            old_region,
            new_region
        );

        let (&addr, &region) = self.regions.range(0..range.end).rev().next().unwrap();
        if addr > range.start {
            panic!("there's a discontinuity in range {:?}", range);
        } else if region != old_region {
            panic!(
                "range {:?} is {:?} (expected {:?})",
                range, region, old_region
            );
        }

        // Insert an element at `range.start`
        if addr == range.start {
            *self.regions.get_mut(&addr).unwrap() = new_region;
        } else {
            self.regions.insert(range.start, new_region);
        }

        // Each element must represent a discontinuity. If it doesnt't represent
        // a discontinuity, it must be removed.
        if let Some((_, &region)) = self.regions.range(0..range.start).rev().next() {
            if region == new_region {
                self.regions.remove(&range.start);
            }
        }

        if let Some(&end_region) = self.regions.get(&range.end) {
            // Each element must represent a discontinuity. If it doesnt't
            // represent a discontinuity, it must be removed.
            if end_region == new_region {
                self.regions.remove(&range.end);
            }
        } else {
            // Insert an element at `range.end`
            self.regions.insert(range.end, old_region);
        }
    }

    pub fn insert_free_block(&mut self, start: NonNull<u8>, end: NonNull<u8>) {
        self.convert_range(
            start.as_ptr() as usize..end.as_ptr() as usize,
            SaRegion::Invalid,
            SaRegion::Free,
        );
    }

    pub fn allocate(&mut self, layout: Layout, start: NonNull<u8>) {
        let start = start.as_ptr() as usize;
        let len = layout.size();
        assert!(
            start % layout.align() == 0,
            "0x{:x} is not properly aligned (0x{:x} bytes alignment required)",
            start,
            layout.align()
        );
        self.convert_range(start..start + len, SaRegion::Free, SaRegion::Used);
    }

    pub fn deallocate(&mut self, layout: Layout, start: NonNull<u8>) {
        let start = start.as_ptr() as usize;
        let len = layout.size();
        assert!(
            start % layout.align() == 0,
            "0x{:x} is not properly aligned (0x{:x} bytes alignment required)",
            start,
            layout.align()
        );
        self.convert_range(start..start + len, SaRegion::Used, SaRegion::Free);
    }
}
