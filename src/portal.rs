pub mod event;

pub trait PortalFront {
    type Event;

    /// Poll the portal for events.
    ///
    /// This is called after the underlying [`Future`](std::future::Future) has been triggered.
    fn poll(&mut self) -> Option<Self::Event>;
}

pub trait PortalBack {
    type Front: PortalFront;

    /// Creates a new portal.
    fn new_portal() -> (Self::Front, Self);
}
