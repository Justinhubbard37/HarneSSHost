#[cfg(windows)]
pub(crate) mod controller;
pub(crate) mod diagnostics;
pub(crate) mod domain;
#[cfg(windows)]
pub(crate) mod presentation;
pub(crate) mod readiness;
#[cfg(windows)]
pub(crate) mod windows;
#[cfg(all(test, windows))]
mod windows_tests;
