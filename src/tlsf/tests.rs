extern crate std;

use quickcheck_macros::quickcheck;
use std::{collections::BTreeMap, ops::Range, prelude::v1::*};

use super::*;

struct ShadowAllocator {
    regions: BTreeMap<usize, SaRegion>,
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
enum SaRegion {
    Free,
    Used,
    Invalid,
}

impl ShadowAllocator {
    fn new() -> Self {
        Self {
            regions: Some((0, SaRegion::Invalid)).into_iter().collect(),
        }
    }

    fn convert_range(&mut self, range: Range<usize>, old_region: SaRegion, new_region: SaRegion) {
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

    fn insert_free_block<T>(&mut self, range: *const [T]) {
        let start = range as *const T as usize;
        let len = unsafe { &*range }.len();
        self.convert_range(start..start + len, SaRegion::Invalid, SaRegion::Free);
    }

    fn allocate(&mut self, layout: Layout, start: NonNull<u8>) {
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

    fn deallocate(&mut self, layout: Layout, start: NonNull<u8>) {
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

#[repr(align(64))]
struct Align<T>(T);

macro_rules! gen_test {
    ($mod:ident, $($tt:tt)*) => {
        mod $mod {
            use super::*;
            type TheTlsf<'a> = Tlsf<'a, $($tt)*>;

            #[test]
            fn minimal() {
                let _ = env_logger::builder().is_test(true).try_init();

                let mut tlsf: TheTlsf = Tlsf::INIT;

                let mut pool = [MaybeUninit::uninit(); 65536];
                tlsf.insert_free_block(&mut pool);

                log::trace!("tlsf = {:?}", tlsf);

                let ptr = tlsf.allocate(Layout::from_size_align(1, 1).unwrap());
                log::trace!("ptr = {:?}", ptr);
                if let Some(ptr) = ptr {
                    unsafe { tlsf.deallocate(ptr, 1) };
                }
            }

            #[test]
            fn adaa() {
                let _ = env_logger::builder().is_test(true).try_init();

                let mut tlsf: TheTlsf = Tlsf::INIT;

                let mut pool = [MaybeUninit::uninit(); 65536];
                tlsf.insert_free_block(&mut pool);

                log::trace!("tlsf = {:?}", tlsf);

                let ptr = tlsf.allocate(Layout::from_size_align(0, 1).unwrap());
                log::trace!("ptr = {:?}", ptr);
                if let Some(ptr) = ptr {
                    unsafe { tlsf.deallocate(ptr, 1) };
                }

                let ptr = tlsf.allocate(Layout::from_size_align(0, 1).unwrap());
                log::trace!("ptr = {:?}", ptr);

                let ptr = tlsf.allocate(Layout::from_size_align(0, 1).unwrap());
                log::trace!("ptr = {:?}", ptr);
            }

            #[test]
            fn insert_free_block_ptr_near_end_fail() {
                let mut tlsf: TheTlsf = Tlsf::INIT;
                unsafe {
                    // FIXME: Use `NonNull::<[T]>::slice_from_raw_parts` when it's stable
                    tlsf.insert_free_block_ptr(
                        NonNull::new(core::ptr::slice_from_raw_parts_mut(
                            (usize::MAX - GRANULARITY + 1) as _,
                            0,
                        ))
                        .unwrap(),
                    );
                }

                // TODO: Allocation should fail
            }

            #[test]
            fn insert_free_block_ptr_near_end() {
                let _tlsf: TheTlsf = Tlsf::INIT;
                // TODO: Find a way to test this case
                //
                // unsafe {
                //     tlsf.insert_free_block_ptr(core::ptr::slice_from_raw_parts_mut(
                //         usize::MAX - GRANULARITY as _,
                //         GRANULARITY,
                //     ));
                // }
            }

            #[quickcheck]
            fn random(pool_start: usize, pool_size: usize, bytecode: Vec<u8>) {
                random_inner(pool_start, pool_size, bytecode);
            }

            fn random_inner(pool_start: usize, pool_size: usize, bytecode: Vec<u8>) -> Option<()> {
                let mut sa = ShadowAllocator::new();
                let mut tlsf: TheTlsf = Tlsf::INIT;

                let mut pool = Align([MaybeUninit::uninit(); 65536]);
                let pool_start = pool_start % 64;
                let pool_size = pool_size % (pool.0.len() - 63);
                let pool = &mut pool.0[pool_start..pool_start+pool_size ];
                log::trace!("pool = {:p}: [u8; {}]", pool, pool.len());
                sa.insert_free_block(pool);
                tlsf.insert_free_block(pool);

                log::trace!("tlsf = {:?}", tlsf);

                #[derive(Debug)]
                struct Alloc {
                    ptr: NonNull<u8>,
                    layout: Layout,
                }
                let mut allocs = Vec::new();

                let mut it = bytecode.iter().cloned();
                loop {
                    match it.next()? % 2 {
                        0 => {
                            let len = u32::from_le_bytes([
                                it.next()?,
                                it.next()?,
                                it.next()?,
                                0,
                            ]);
                            let len = ((len as u64 * pool_size as u64) >> 24) as usize;
                            let align = 1 << (it.next()? % 6);
                            let layout = Layout::from_size_align(len, align).unwrap();
                            log::trace!("alloc {:?}", layout);

                            let ptr = tlsf.allocate(layout);
                            log::trace!(" → {:?}", ptr);

                            if let Some(ptr) = ptr {
                                allocs.push(Alloc { ptr, layout });
                                sa.allocate(layout, ptr);
                            }
                        }
                        1 => {
                            let alloc_i = it.next()?;
                            if allocs.len() > 0 {
                                let alloc = allocs.swap_remove(alloc_i as usize % allocs.len());
                                log::trace!("dealloc {:?}", alloc);

                                unsafe { tlsf.deallocate(alloc.ptr, alloc.layout.align()) };
                                sa.deallocate(alloc.layout, alloc.ptr);
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            }
        }
    };
}

gen_test!(tlsf_u8_u8_1_1, u8, u8, 1, 1);
gen_test!(tlsf_u8_u8_1_2, u8, u8, 1, 2);
gen_test!(tlsf_u8_u8_1_4, u8, u8, 1, 4);
gen_test!(tlsf_u8_u8_1_8, u8, u8, 1, 8);
gen_test!(tlsf_u8_u8_3_4, u8, u8, 3, 4);
gen_test!(tlsf_u8_u8_5_4, u8, u8, 5, 4);
gen_test!(tlsf_u8_u8_8_1, u8, u8, 8, 1);
gen_test!(tlsf_u8_u8_8_8, u8, u8, 8, 8);
gen_test!(tlsf_u16_u8_3_4, u16, u8, 3, 4);
gen_test!(tlsf_u16_u8_11_4, u16, u8, 11, 4);
gen_test!(tlsf_u16_u8_16_4, u16, u8, 16, 4);
gen_test!(tlsf_u16_u16_3_16, u16, u16, 3, 16);
gen_test!(tlsf_u16_u16_11_16, u16, u16, 11, 16);
gen_test!(tlsf_u16_u16_16_16, u16, u16, 16, 16);
gen_test!(tlsf_u16_u32_3_16, u16, u32, 3, 16);
gen_test!(tlsf_u16_u32_11_16, u16, u32, 11, 16);
gen_test!(tlsf_u16_u32_16_16, u16, u32, 16, 16);
gen_test!(tlsf_u16_u32_3_32, u16, u32, 3, 32);
gen_test!(tlsf_u16_u32_11_32, u16, u32, 11, 32);
gen_test!(tlsf_u16_u32_16_32, u16, u32, 16, 32);
gen_test!(tlsf_u32_u32_20_32, u32, u32, 20, 32);
