extern crate std;

use quickcheck_macros::quickcheck;
use std::{collections::BTreeMap, mem::MaybeUninit, ops::Range, prelude::v1::*};

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
            fn aadd() {
                let _ = env_logger::builder().is_test(true).try_init();

                let mut tlsf: TheTlsf = Tlsf::INIT;

                let mut pool = Align([MaybeUninit::uninit(); 96]);
                tlsf.insert_free_block(&mut pool.0);

                log::trace!("tlsf = {:?}", tlsf);

                let ptr1 = tlsf.allocate(Layout::from_size_align(0, 1).unwrap());
                log::trace!("ptr1 = {:?}", ptr1);

                let ptr2 = tlsf.allocate(Layout::from_size_align(0, 1).unwrap());
                log::trace!("ptr2 = {:?}", ptr2);

                if let (Some(ptr1), Some(ptr2)) = (ptr1, ptr2) {
                    unsafe { tlsf.deallocate(ptr1, 1) };
                    unsafe { tlsf.deallocate(ptr2, 1) };
                }
            }

            #[test]
            fn ara() {
                let _ = env_logger::builder().is_test(true).try_init();

                let mut tlsf: TheTlsf = Tlsf::INIT;

                let mut pool = Align([MaybeUninit::uninit(); 96]);
                tlsf.insert_free_block(&mut pool.0);

                log::trace!("tlsf = {:?}", tlsf);

                let ptr = tlsf.allocate(Layout::from_size_align(17, 1).unwrap());
                log::trace!("ptr = {:?}", ptr);

                if let Some(ptr) = ptr {
                    unsafe { tlsf.reallocate(ptr, Layout::from_size_align(0, 1).unwrap()) };
                    log::trace!("ptr = {:?}", ptr);
                }

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
                    match it.next()? % 8 {
                        0..=2 => {
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
                        3..=5 => {
                            let alloc_i = it.next()?;
                            if allocs.len() > 0 {
                                let alloc = allocs.swap_remove(alloc_i as usize % allocs.len());
                                log::trace!("dealloc {:?}", alloc);

                                unsafe { tlsf.deallocate(alloc.ptr, alloc.layout.align()) };
                                sa.deallocate(alloc.layout, alloc.ptr);
                            }
                        }
                        6..=7 => {
                            let alloc_i = it.next()?;
                            if allocs.len() > 0 {
                                let len = u32::from_le_bytes([
                                    it.next()?,
                                    it.next()?,
                                    it.next()?,
                                    0,
                                ]);
                                let len = ((len as u64 * pool_size as u64) >> 24) as usize;

                                let alloc_i = alloc_i as usize % allocs.len();
                                let alloc = &mut allocs[alloc_i];
                                log::trace!("realloc {:?} to {:?}", alloc, len);

                                let new_layout = Layout::from_size_align(len, alloc.layout.align()).unwrap();

                                if let Some(ptr) = unsafe { tlsf.reallocate(alloc.ptr, new_layout) } {
                                    log::trace!(" {:?} → {:?}", alloc.ptr, ptr);
                                    sa.deallocate(alloc.layout, alloc.ptr);
                                    alloc.ptr = ptr;
                                    alloc.layout = new_layout;
                                    sa.allocate(alloc.layout, alloc.ptr);
                                } else {
                                    log::trace!(" {:?} → fail", alloc.ptr);

                                }
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            }

            #[test]
            fn max_pool_size() {
                if let Some(mps) = TheTlsf::MAX_POOL_SIZE {
                    // `MAX_POOL_SIZE - super::GRANULARITY` should
                    // be the maximum allowed block size.
                    assert!(TheTlsf::map_floor(mps - super::GRANULARITY).is_some());
                    assert_eq!(TheTlsf::map_floor(mps), None);
                }
            }

            #[quickcheck]
            fn map_ceil_and_unmap(size: usize, shift: u32) -> quickcheck::TestResult {
                let size = size.rotate_left(shift % super::USIZE_BITS)
                    .wrapping_mul(super::GRANULARITY);
                if size == 0 {
                    return quickcheck::TestResult::discard();
                }
                let list_min_size = TheTlsf::map_ceil_and_unmap(size);
                log::debug!("map_ceil_and_unmap({}) = {:?}", size, list_min_size);
                if let Some(list_min_size) = list_min_size {
                    assert!(list_min_size >= size);

                    // `list_min_size` must be the lower bound of some list
                    let (fl, sl) = TheTlsf::map_floor(list_min_size).unwrap();
                    log::debug!("map_floor({}) = {:?}", list_min_size, (fl, sl));

                    // Since `list_min_size` is the lower bound of some list,
                    // `map_floor(list_min_size)` and `map_ceil(list_min_size)`
                    // should both return this list
                    assert_eq!(TheTlsf::map_floor(list_min_size), TheTlsf::map_ceil(list_min_size));

                    // `map_ceil_and_unmap(size)` must be the lower bound of the
                    // list returned by `map_ceil(size)`
                    assert_eq!(TheTlsf::map_floor(list_min_size), TheTlsf::map_ceil(size));
                } else {
                    // Find an explanation for `map_ceil_and_unmap` returning
                    // `None`
                    if let Some((fl, _sl)) = TheTlsf::map_ceil(size) {
                        // The lower bound of `(fl, sl)` is not representable
                        // in `usize` - this should be why
                        assert!(fl as u32 + super::GRANULARITY_LOG2 >= super::USIZE_BITS);
                    } else {
                        // `map_ceil_and_unmap` is `map_ceil` + infallible
                        // reverse mapping, and the suboperation `map_ceil`
                        // failed
                    }
                }

                quickcheck::TestResult::passed()
            }

            #[quickcheck]
            fn map_ceil_and_unmap_huge(shift: u32) -> quickcheck::TestResult {
                let size = usize::MAX <<
                    (shift % (super::USIZE_BITS - super::GRANULARITY_LOG2)
                        + super::GRANULARITY_LOG2);

                if size == 0 || TheTlsf::map_ceil(size).is_some() {
                    return quickcheck::TestResult::discard();
                }

                // If `map_ceil` returns `None`, `map_ceil_and_unmap` must
                // return `None`, too.
                assert_eq!(TheTlsf::map_ceil_and_unmap(size), None);
                quickcheck::TestResult::passed()
            }

            #[quickcheck]
            fn pool_size_to_contain_allocation(size: usize, align: u32)-> quickcheck::TestResult {
                let align = (super::GRANULARITY / 2) << (align % 5);
                let size = size.wrapping_mul(align);
                if size > 500_000 {
                    // Let's limit pool size
                    return quickcheck::TestResult::discard();
                }

                let layout = Layout::from_size_align(size, align).unwrap();
                log::debug!("layout = {:?}", layout);

                let pool_size = if let Some(x) = TheTlsf::pool_size_to_contain_allocation(layout) {
                    x
                } else {
                    return quickcheck::TestResult::discard();
                };
                log::debug!("pool_size_to_contain_allocation = {:?}", pool_size);

                assert_eq!(pool_size % super::GRANULARITY, 0);

                // Create a well-aligned pool
                type Bk = Align<[u8; 64]>;
                assert_eq!(std::mem::size_of::<Bk>(), 64);
                assert_eq!(std::mem::align_of::<Bk>(), 64);
                let mut pool: Vec<MaybeUninit<Bk>> = Vec::new();
                pool.reserve((pool_size + 63) / 64);
                let pool = unsafe {
                    std::slice::from_raw_parts_mut(
                        pool.as_mut_ptr() as *mut MaybeUninit<u8>,
                        pool_size,
                    )
                };

                let mut tlsf: TheTlsf = Tlsf::INIT;
                tlsf.insert_free_block(pool);

                // The allocation should success because
                // `pool_size_to_contain_allocation` said so
                tlsf.allocate(layout)
                    .expect("allocation unexpectedly failed");

                quickcheck::TestResult::passed()
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
gen_test!(tlsf_u32_u32_27_32, u32, u32, 27, 32);
gen_test!(tlsf_u32_u32_28_32, u32, u32, 28, 32);
gen_test!(tlsf_u32_u32_29_32, u32, u32, 29, 32);
gen_test!(tlsf_u32_u32_32_32, u32, u32, 32, 32);
gen_test!(tlsf_u64_u8_58_64, u64, u64, 58, 8);
gen_test!(tlsf_u64_u8_59_64, u64, u64, 59, 8);
gen_test!(tlsf_u64_u8_60_64, u64, u64, 60, 8);
gen_test!(tlsf_u64_u8_61_64, u64, u64, 61, 8);
gen_test!(tlsf_u64_u8_64_64, u64, u64, 64, 8);
