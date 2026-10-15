//! Interruptible computations.

/// Something (typically a worker thread) that can be asked to stop.
///
/// Port of the Java interface `app.freerouting.datastructures.Stoppable`.
pub trait Stoppable {
    /// Requests this computation to be stopped.
    fn request_stop(&self);

    /// True if a stop of this computation was requested.
    fn is_stop_requested(&self) -> bool;
}
