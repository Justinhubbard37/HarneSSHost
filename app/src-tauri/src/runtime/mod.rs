#[allow(dead_code)]
// Pure bounded infrastructure for later runtime gates; Gate 4B-1 does not execute it.
pub(crate) mod diagnostics;
#[allow(dead_code)] // Gate 4B-3 transition producers remain internal.
pub(crate) mod domain;
#[allow(dead_code)] // Readiness remains private to the internal runtime wiring.
pub(crate) mod readiness;
#[cfg(windows)]
#[allow(dead_code)] // Gate 4B-3 runtime wiring remains internal; no product command is exposed.
pub(crate) mod windows;
#[cfg(all(test, windows))]
mod windows_tests;
