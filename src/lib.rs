#![no_std]

// FIXME: panicking in constants is unstable
macro_rules! const_panic {
    ($($tt:tt)*) => {
        #[allow(unconditional_panic)]
        {
            let _ = 1 / 0;
            loop {}
        }
    };
}

pub mod int;
mod tlsf;
pub use self::tlsf::{Tlsf, GRANULARITY};

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
