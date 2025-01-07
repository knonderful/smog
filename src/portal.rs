pub mod event;

pub trait Portal {
    type Front;
    type Back;

    /// Creates a new portal.
    fn new() -> (Self::Front, Self::Back);
}

pub trait PortalFront {
    type Event;

    /// Poll the portal for events.
    ///
    /// This is called after the underlying [`Future`](std::future::Future) has been triggered.
    fn poll(&mut self) -> Option<Self::Event>;
}
