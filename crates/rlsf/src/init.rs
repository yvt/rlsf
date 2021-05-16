/// Provides a constant default value.
pub trait Init {
    /// `Self`'s default value.
    const INIT: Self;
}

#[cfg(any(test, feature = "std"))]
#[cfg_attr(feature = "doc_cfg", doc(cfg(feature = "std")))]
impl Init for std::alloc::System {
    const INIT: Self = Self;
}
