# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](http://keepachangelog.com/en/1.0.0/)
and this project adheres to [Semantic Versioning](http://semver.org/spec/v2.0.0.html).

## [Unreleased]

- **Added:** `Tlsf::iter_blocks` lets you enumerate memory blocks for debugging purposes.

## [0.1.2] - 2021-05-30

- Performance and code size optimization
- **Added:** `GlobalTlsf` now provides a `malloc`-compatible interface.
- **Fixed:** Raised the version requirement of `libc` to 0.2.56, where `MAP_FIXED_NOREPLACE` was added.

## [0.1.1] - 2021-05-23

- **Added:** `GlobalTlsf` now supports POSIX-compliant systems (`cfg(unix)`).
- **Fixed:** Addressed a bug in `Tlsf::reallocate` that caused an incorrect amount of data to be copied (possibly corrupting memory or crashing the program) during a moving reallocation.

## 0.1.0 - 2021-05-21

- Initial release.

[Unreleased]: https://github.com/yvt/rlsf/compare/0.1.2...HEAD
[0.1.2]: https://github.com/yvt/rlsf/compare/0.1.1...0.1.2
[0.1.1]: https://github.com/yvt/rlsf/compare/0.1.0...0.1.1
