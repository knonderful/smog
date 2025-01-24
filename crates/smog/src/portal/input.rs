//! A portal implementation that supports input events. See [`InBack`] and [`InFront`].

use crate::portal::{PortalBack, PortalFront};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

enum InState<I> {
    /// Neutral state.
    Neutral,
    /// The back side is awaiting an event.
    AwaitingSignalled,
    /// The coroutine has confirmed that the back is awaiting.
    AwaitingConfirmed,
    /// The front side has provided an event.
    FrontProvided(I),
    /// The portal is closed. This happens when either side of the portal is dropped.
    Closed,
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum InEvent {
    /// The back side is awaiting an event.
    Awaiting,
}

/// The [`PortalFront`] implementation.
pub struct InFront<I> {
    state: InState<I>,
}

impl<I> Drop for InFront<I> {
    fn drop(&mut self) {
        self.state = InState::Closed;
    }
}

impl<I> InFront<I> {
    pub fn provide(&mut self, event: I) {
        match &self.state {
            InState::Neutral | InState::AwaitingConfirmed => {} // OK: fall-through
            InState::AwaitingSignalled => panic!("Providing a new input while state is AwaitingSignalled."),
            InState::FrontProvided(_) => panic!("Providing a new input event on a non-empty slot. Make sure to consume provided values in the coroutine before providing new ones."),
            InState::Closed => panic!("Providing an input event on a closed portal."),
        }

        self.state = InState::FrontProvided(event);
    }
}

impl<I> PortalFront for InFront<I> {
    type Event = InEvent;

    fn poll(self: Pin<&mut Self>) -> Option<Self::Event> {
        // SAFETY: We're treating this as pinned.
        let this = unsafe { self.get_unchecked_mut() };
        match this.state {
            InState::AwaitingSignalled => {} // fall-through
            InState::Neutral | InState::AwaitingConfirmed | InState::FrontProvided(_) | InState::Closed => return None,
        }

        this.state = InState::AwaitingConfirmed;
        Some(InEvent::Awaiting)
    }
}

impl<I> Default for InFront<I> {
    fn default() -> Self {
        Self {
            state: InState::Neutral,
        }
    }
}

/// The [`PortalBack`] implementation.
pub struct InBack<I> {
    state: *mut InState<I>,
}

impl<I> Drop for InBack<I> {
    fn drop(&mut self) {
        unsafe {
            *self.state = InState::Closed;
        }
    }
}

impl<I> InBack<I> {
    /// Receives an event from the outside of the coroutine.
    pub fn receive(&mut self) -> impl Future<Output = I> + use<'_, I> {
        unsafe { ReceiveFuture::new(&mut *self.state) }
    }
}

impl<I> PortalBack for InBack<I>
where
    I: Unpin,
{
    type Front = InFront<I>;

    fn new(mut front: Pin<&mut Self::Front>) -> Self {
        Self {
            state: &mut front.state as *mut _,
        }
    }
}

struct ReceiveFuture<'a, I> {
    state: &'a mut InState<I>,
}

impl<'a, I> ReceiveFuture<'a, I> {
    fn new(state: &'a mut InState<I>) -> Self {
        match state {
            InState::Neutral => *state = InState::AwaitingSignalled,
            InState::AwaitingSignalled => panic!("Started receive while state is BackAwaiting."),
            InState::AwaitingConfirmed => {
                panic!("Started receive while state is AwaitingConfirmed.")
            }
            InState::FrontProvided(_) => {} // fall-through
            InState::Closed => panic!("Started receive while state is Closed."),
        }

        Self { state }
    }
}

impl<I> Future for ReceiveFuture<'_, I> {
    type Output = I;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &self.state {
            InState::AwaitingSignalled | InState::AwaitingConfirmed | InState::Neutral => return Poll::Pending,
            InState::FrontProvided(_) => {} // fall-through
            InState::Closed => panic!("Polling while state is Closed"),
        }

        let state = std::mem::replace(self.state, InState::Neutral);
        let InState::FrontProvided(event) = state else {
            unreachable!()
        };
        Poll::Ready(event)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{catch_unwind_silent, driver, CoroPoll};
    use std::sync::Mutex;

    async fn create_machine(mut portal: InBack<u32>) -> u64 {
        let mut values = Vec::new();
        while values.len() < 2 {
            let value = u64::from(portal.receive().await) * 2;
            values.push(value);
        }

        values.iter().sum()
    }

    #[test]
    fn test_valid() {
        let mut driver = driver().function(create_machine);

        assert_eq!(CoroPoll::Event(InEvent::Awaiting), driver.poll());
        driver.portal().provide(150);
        assert_eq!(CoroPoll::Event(InEvent::Awaiting), driver.poll());
        driver.portal().provide(52);
        assert_eq!(CoroPoll::Result(404), driver.poll());
    }

    /// Like `test_valid`, but without the poll-before-provide.
    #[test]
    fn test_valid_without_prepoll() {
        let mut driver = driver().function(create_machine);

        driver.portal().provide(150);
        assert_eq!(CoroPoll::Event(InEvent::Awaiting), driver.poll());
        driver.portal().provide(52);
        assert_eq!(CoroPoll::Result(404), driver.poll());
    }

    #[test]
    fn test_poll_after_completion() {
        let mut driver = driver().function(create_machine);

        assert_eq!(CoroPoll::Event(InEvent::Awaiting), driver.poll());
        driver.portal().provide(150);
        assert_eq!(CoroPoll::Event(InEvent::Awaiting), driver.poll());
        driver.portal().provide(52);
        assert_eq!(CoroPoll::Result(404), driver.poll());

        let mut driver = Mutex::new(driver);
        // Trying to poll after completion should panic.
        let result = catch_unwind_silent(move || driver.get_mut().unwrap().poll());
        assert!(result.is_err());
    }

    #[test]
    fn test_provide_without_await() {
        let mut driver = driver().function(create_machine);

        driver.portal().provide(150);
        assert_eq!(CoroPoll::Event(InEvent::Awaiting), driver.poll());
        driver.portal().provide(52);
        assert_eq!(CoroPoll::Result(404), driver.poll());
    }

    #[test]
    fn test_poll_after_await() {
        let mut driver = driver().function(create_machine);

        assert_eq!(CoroPoll::Event(InEvent::Awaiting), driver.poll());

        let mut driver = Mutex::new(driver);
        // Trying to poll again without providing input will cause the portal to not emit an event,
        // which should cause `Driver` to panic.
        let result = catch_unwind_silent(move || driver.get_mut().unwrap().poll());
        assert!(result.is_err());
    }
}
