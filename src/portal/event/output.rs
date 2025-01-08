use crate::cell::OptimizedRefCell;
use crate::portal::{Portal, PortalFront};
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

/// A [`Portal`] implementation that supports output events (from the coroutine).
pub struct OutPortal<O>(PhantomData<O>);

impl<O> Portal for OutPortal<O> {
    type Front = OutFront<O>;
    type Back = OutBack<O>;

    fn new() -> (Self::Front, Self::Back) {
        let state = Rc::new(OptimizedRefCell::new(OutState::Neutral));
        (Self::Front::new(state.clone()), Self::Back::new(state))
    }
}

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

pub struct OutFront<O> {
    state: Rc<OptimizedRefCell<OutState<O>>>,
}

impl<O> Drop for OutFront<O> {
    fn drop(&mut self) {
        *self.state.borrow_mut() = OutState::Closed;
    }
}

impl<O> OutFront<O> {
    fn new(state: Rc<OptimizedRefCell<OutState<O>>>) -> Self {
        Self { state }
    }
}

impl<O> PortalFront for OutFront<O> {
    type Event = OutEvent<O>;

    fn poll(&mut self) -> Option<Self::Event> {
        match *self.state.borrow() {
            OutState::Yielded(_) => {} // fall-through
            OutState::Neutral | OutState::Closed => return None,
        }

        let OutState::Yielded(event) = self.state.replace(OutState::Neutral) else {
            unreachable!()
        };

        Some(OutEvent::Yielded(event))
    }
}

pub struct OutBack<O> {
    state: Rc<OptimizedRefCell<OutState<O>>>,
}

impl<O> Drop for OutBack<O> {
    fn drop(&mut self) {
        *self.state.borrow_mut() = OutState::Closed;
    }
}

impl<O> OutBack<O> {
    fn new(state: Rc<OptimizedRefCell<OutState<O>>>) -> Self {
        Self { state }
    }

    /// Sends an event out of the coroutine.
    pub fn send(&mut self, event: O) -> impl Future<Output = ()> + use<'_, O> {
        SendFuture::new(self.state.as_ref(), event)
    }
}

struct SendFuture<'a, O> {
    state: &'a OptimizedRefCell<OutState<O>>,
}

impl<'a, O> SendFuture<'a, O> {
    fn new(state: &'a OptimizedRefCell<OutState<O>>, event: O) -> Self {
        match *state.borrow() {
            OutState::Neutral => {} // OK: fall-through
            OutState::Yielded(_) => panic!("Started send while state is Yielded."),
            OutState::Closed => panic!("Started send while state is Closed."),
        }

        *state.borrow_mut() = OutState::Yielded(event);

        Self { state }
    }
}

impl<O> Future for SendFuture<'_, O> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match *self.state.borrow() {
            OutState::Yielded(_) => Poll::Pending,
            OutState::Neutral => Poll::Ready(()),
            OutState::Closed => panic!("Polling while state is Closed"),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{catch_unwind_silent, Coro, CoroPoll};
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
        let (front, back) = OutPortal::new();
        let future = pin!(create_machine(back, &[150, 52, 666]));
        let mut coro = Coro::new(front, future);

        assert_eq!(CoroPoll::Event(OutEvent::Yielded(300)), coro.poll());
        assert_eq!(CoroPoll::Event(OutEvent::Yielded(104)), coro.poll());
        assert_eq!(CoroPoll::Result(404), coro.poll());
    }

    #[test]
    fn test_poll_after_completion() {
        let (front, back) = OutPortal::new();
        let future = pin!(create_machine(back, &[150, 52, 666]));
        let mut coro = Coro::new(front, future);

        assert_eq!(CoroPoll::Event(OutEvent::Yielded(300)), coro.poll());
        assert_eq!(CoroPoll::Event(OutEvent::Yielded(104)), coro.poll());
        assert_eq!(CoroPoll::Result(404), coro.poll());

        let mut coro = Mutex::new(coro);
        // Trying to poll after completion should panic.
        let result = catch_unwind_silent(move || coro.get_mut().unwrap().poll());
        assert!(result.is_err());
    }
}
