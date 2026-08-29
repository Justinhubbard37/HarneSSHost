#[allow(dead_code)]
// Pure bounded infrastructure for later runtime gates; Gate 4B-1 does not execute it.
pub(crate) mod diagnostics;
#[allow(dead_code)] // Transition producers arrive in later gates.
pub(crate) mod domain;
#[allow(dead_code)] // Parsing is intentionally not connected to a process in Gate 4B-1.
pub(crate) mod readiness;
