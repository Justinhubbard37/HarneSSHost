#[cfg(windows)]
pub(crate) mod controller;
#[cfg(windows)]
pub(crate) mod deepseek_driver;
pub(crate) mod diagnostics;
pub(crate) mod domain;
#[cfg(windows)]
pub(crate) mod driver;
#[cfg(windows)]
pub(crate) mod presentation;
pub(crate) mod readiness;
#[cfg(windows)]
pub(crate) mod windows;
#[cfg(all(test, windows))]
mod windows_tests;
