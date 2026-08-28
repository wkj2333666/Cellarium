//! Cellarium: a local application for designing and running lattice
//! experiments. Everything runs in this process, in its own window.

pub mod cli;
pub mod document;
pub mod gui;
pub mod render;
pub mod sim;

/// Serialization for tests that build a real compute backend.
///
/// Constructing one probes the machine's devices for real. Several probes
/// racing each other inside a single test process can wedge a desktop GPU
/// driver: the suite was seen to stop dead for twenty-five minutes, every
/// thread parked on a futex with the vendor's Vulkan threads among them, and it
/// had to be killed. Cellarium itself only ever runs one worker, so holding this
/// while a test starts one tests the shape the product actually has rather than
/// a concurrency it never creates.
#[cfg(test)]
pub(crate) mod test_backend_guard {
    use std::sync::{Mutex, MutexGuard};

    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    /// Hold this for as long as a real backend exists.
    ///
    /// A poisoning left by an unrelated failing test is ignored: the guard
    /// protects a driver, not data, and refusing to hand it over would turn one
    /// failure into a cascade.
    pub(crate) fn one_backend_at_a_time() -> MutexGuard<'static, ()> {
        ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|held| held.into_inner())
    }
}
