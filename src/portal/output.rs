//! A portal implementation that supports output events. See [`OutBack`] and [`OutFront`].

use crate::portal::{PortalBack, PortalFront};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

enum OutState<O> {
    /// Neutral state.
    Neutral,
    /// The back side has yielded an event.
    Yielded(O),
    /// The portal is closed. This happens when either side of the portal is dropped.
    Closed,
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum OutEvent<O> {
    /// The back side has yielded an event.
    Yielded(O),
}

/// The [`PortalFront`] implementation.
pub struct OutFront<O> {
    state: OutState<O>,
}

impl<O> Drop for OutFront<O> {
    fn drop(&mut self) {
        self.state = OutState::Closed;
    }
}

impl<O> PortalFront for OutFront<O> {
    type Event = OutEvent<O>;

    fn poll(self: Pin<&mut Self>) -> Option<Self::Event> {
        // SAFETY: We're treating this as pinned.
        let this = unsafe { self.get_unchecked_mut() };
        match this.state {
            OutState::Yielded(_) => {} // fall-through
            OutState::Neutral | OutState::Closed => return None,
        }

        let state = std::mem::replace(&mut this.state, OutState::Neutral);
        let OutState::Yielded(event) = state else {
            unreachable!()
        };

        Some(OutEvent::Yielded(event))
    }
}

impl<O> Default for OutFront<O> {
    fn default() -> Self {
        Self {
            state: OutState::Neutral,
        }
    }
}

/// The [`PortalBack`] implementation.
pub struct OutBack<O> {
    state: *mut OutState<O>,
}

impl<O> Drop for OutBack<O> {
    fn drop(&mut self) {
        unsafe {
            *self.state = OutState::Closed;
        }
    }
}

impl<O> OutBack<O> {
    /// Sends an event out of the coroutine.
    pub fn send(&mut self, event: O) -> impl Future<Output = ()> + use<'_, O> {
        unsafe { SendFuture::new(&mut *self.state, event) }
    }
}

impl<O> PortalBack for OutBack<O>
where
    O: Unpin,
{
    type Front = OutFront<O>;

    fn new(mut front: Pin<&mut Self::Front>) -> Self {
        Self {
            state: &mut front.state as *mut _,
        }
    }
}

struct SendFuture<'a, O> {
    state: &'a mut OutState<O>,
}

impl<'a, O> SendFuture<'a, O> {
    fn new(state: &'a mut OutState<O>, event: O) -> Self {
        match state {
            OutState::Neutral => {} // OK: fall-through
            OutState::Yielded(_) => panic!("Started send while state is Yielded."),
            OutState::Closed => panic!("Started send while state is Closed."),
        }

        *state = OutState::Yielded(event);

        Self { state }
    }
}

impl<O> Future for SendFuture<'_, O> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.state {
            OutState::Yielded(_) => Poll::Pending,
            OutState::Neutral => Poll::Ready(()),
            OutState::Closed => panic!("Polling while state is Closed"),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{catch_unwind_silent, driver, CoroPoll};
    use std::pin::pin;
    use std::sync::Mutex;

    async fn create_machine(mut portal: OutBack<u64>, input: &[u32]) -> u64 {
        let mut values = Vec::new();
        for value in input.iter().take(2) {
            let value = u64::from(*value) * 2;
            portal.send(value).await;
            values.push(value);
        }

        values.iter().sum()
    }

    #[test]
    fn test_valid() {
        let mut driver = pin!(driver(|back| create_machine(back, &[150, 52, 666])));

        assert_eq!(CoroPoll::Event(OutEvent::Yielded(300)), driver.poll());
        assert_eq!(CoroPoll::Event(OutEvent::Yielded(104)), driver.poll());
        assert_eq!(CoroPoll::Result(404), driver.poll());
    }

    #[test]
    fn test_poll_after_completion() {
        let mut driver = pin!(driver(|back| create_machine(back, &[150, 52, 666])));

        assert_eq!(CoroPoll::Event(OutEvent::Yielded(300)), driver.poll());
        assert_eq!(CoroPoll::Event(OutEvent::Yielded(104)), driver.poll());
        assert_eq!(CoroPoll::Result(404), driver.poll());

        let mut driver = Mutex::new(driver);
        // Trying to poll after completion should panic.
        let result = catch_unwind_silent(move || driver.get_mut().unwrap().poll());
        assert!(result.is_err());
    }
}
