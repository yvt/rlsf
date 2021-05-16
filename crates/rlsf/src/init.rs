/// Provides a constant default value.
pub trait Init {
    /// `Self`'s default value.
    const INIT: Self;
}

#[cfg(any(test, feature = "std"))]
impl Init for std::alloc::System {
    const INIT: Self = Self;
}
