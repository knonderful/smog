//! Portals are the gateway between the inside of the coroutine and the outside world. A portal implementation consists
//! of two parts:
//!
//! - [`PortalBack`]: the side that plugs in to the coroutine.
//! - [`PortalFront`]: the side that plugs into the [`Driver`](crate::Driver).
//!
//! Two implementations form a pair via the [`PortalBack::Front`] assocated type. A portal can be created via
//! [`PortalBack::new_portal()`]. Additionally, the front must implement [`PortalFront`] in order to be able to be
//! plugged into the [`Driver`](crate::Driver).
//!
//! This crate comes with some portal implementations for common use:
//!
//! - [`Input`](input): Allows for events to be sent into the coroutine.
//! - [`Output`](output): Allows for events to be sent out of the coroutine (creating a generator).
//! - [`Input & output`](inout): Allows for events into and out of the coroutine.

pub mod inout;
pub mod input;
pub mod output;

/// The front side of a portal.
pub trait PortalFront {
    type Event;

    /// Poll the portal for events.
    ///
    /// This is called after the underlying [`Future`](std::future::Future) has been triggered.
    ///
    /// # Important
    ///
    /// The [`PortalFront`] implementation must _consume_ the events that are returned via this function call. [`Driver`]
    /// will call this function repeatedly until `None` is returned before advancing the [`Future`].
    fn poll(&mut self) -> Option<Self::Event>;
}

/// The back side of a portal.
pub trait PortalBack {
    /// The type of the back side of the portal.
    type Front: PortalFront;

    /// Creates a new portal.
    fn new_portal() -> (Self::Front, Self);
}
