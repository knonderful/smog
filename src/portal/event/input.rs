use crate::cell::OptimizedRefCell;
use crate::portal::{PortalBack, PortalFront};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
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

pub struct InFront<I> {
    state: Rc<OptimizedRefCell<InState<I>>>,
}

impl<I> Drop for InFront<I> {
    fn drop(&mut self) {
        *self.state.borrow_mut() = InState::Closed;
    }
}

impl<I> InFront<I> {
    fn new(state: Rc<OptimizedRefCell<InState<I>>>) -> Self {
        Self { state }
    }

    pub fn provide(&mut self, event: I) {
        match *self.state.borrow() {
            InState::AwaitingConfirmed => {} // OK: fall-through
            InState::Neutral => panic!("Providing a new input while state is Neutral."),
            InState::AwaitingSignalled => panic!("Providing a new input while state is AwaitingSignalled."),
            InState::FrontProvided(_) => panic!("Providing a new input event on a non-empty slot. Make sure to consume provided values in the coroutine before providing new ones."),
            InState::Closed => panic!("Providing an input event on a closed portal."),
        }

        *self.state.borrow_mut() = InState::FrontProvided(event);
    }
}

impl<I> PortalFront for InFront<I> {
    type Event = InEvent;

    fn poll(&mut self) -> Option<Self::Event> {
        match *self.state.borrow() {
            InState::AwaitingSignalled => {} // fall-through
            InState::Neutral | InState::AwaitingConfirmed | InState::FrontProvided(_) | InState::Closed => return None,
        }

        *self.state.borrow_mut() = InState::AwaitingConfirmed;
        Some(InEvent::Awaiting)
    }
}

pub struct InBack<I> {
    state: Rc<OptimizedRefCell<InState<I>>>,
}

impl<I> Drop for InBack<I> {
    fn drop(&mut self) {
        *self.state.borrow_mut() = InState::Closed;
    }
}

impl<I> InBack<I> {
    fn new(state: Rc<OptimizedRefCell<InState<I>>>) -> Self {
        Self { state }
    }

    /// Receives an event from the outside of the coroutine.
    pub fn receive(&mut self) -> impl Future<Output = I> + use<'_, I> {
        ReceiveFuture::new(self.state.as_ref())
    }
}

impl<I> PortalBack for InBack<I> {
    type Front = InFront<I>;

    fn new_portal() -> (Self::Front, Self) {
        let state = Rc::new(OptimizedRefCell::new(InState::Neutral));
        (Self::Front::new(state.clone()), Self::new(state))
    }
}

struct ReceiveFuture<'a, I> {
    state: &'a OptimizedRefCell<InState<I>>,
}

impl<'a, I> ReceiveFuture<'a, I> {
    fn new(state: &'a OptimizedRefCell<InState<I>>) -> Self {
        match *state.borrow() {
            InState::Neutral => {} // OK: fall-through
            InState::AwaitingSignalled => panic!("Started receive while state is BackAwaiting."),
            InState::AwaitingConfirmed => {
                panic!("Started receive while state is AwaitingConfirmed.")
            }
            InState::FrontProvided(_) => panic!("Started receive while state is FrontProvided."),
            InState::Closed => panic!("Started receive while state is Closed."),
        }

        *state.borrow_mut() = InState::AwaitingSignalled;

        Self { state }
    }
}

impl<I> Future for ReceiveFuture<'_, I> {
    type Output = I;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match *self.state.borrow() {
            InState::AwaitingSignalled | InState::AwaitingConfirmed | InState::Neutral => return Poll::Pending,
            InState::FrontProvided(_) => {} // fall-through
            InState::Closed => panic!("Polling while state is Closed"),
        }

        let InState::FrontProvided(event) = self.state.replace(InState::Neutral) else {
            unreachable!()
        };
        Poll::Ready(event)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{catch_unwind_silent, coro_from_fn, CoroPoll};
    use std::pin::pin;
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
        let mut coro = pin!(coro_from_fn(create_machine));

        assert_eq!(CoroPoll::Event(InEvent::Awaiting), coro.poll());
        coro.portal().provide(150);
        assert_eq!(CoroPoll::Event(InEvent::Awaiting), coro.poll());
        coro.portal().provide(52);
        assert_eq!(CoroPoll::Result(404), coro.poll());
    }

    #[test]
    fn test_poll_after_completion() {
        let mut coro = pin!(coro_from_fn(create_machine));

        assert_eq!(CoroPoll::Event(InEvent::Awaiting), coro.poll());
        coro.portal().provide(150);
        assert_eq!(CoroPoll::Event(InEvent::Awaiting), coro.poll());
        coro.portal().provide(52);
        assert_eq!(CoroPoll::Result(404), coro.poll());

        let mut coro = Mutex::new(coro);
        // Trying to poll after completion should panic.
        let result = catch_unwind_silent(move || coro.get_mut().unwrap().poll());
        assert!(result.is_err());
    }

    #[test]
    fn test_provide_without_await() {
        let coro = pin!(coro_from_fn(create_machine));
        let mut coro = Mutex::new(coro);

        // Trying to provide input without awaiting first (since the coro hasn't been polled yet)
        let result = catch_unwind_silent(move || coro.get_mut().unwrap().portal().provide(150));
        assert!(result.is_err());
    }

    #[test]
    fn test_poll_after_await() {
        let mut coro = pin!(coro_from_fn(create_machine));

        assert_eq!(CoroPoll::Event(InEvent::Awaiting), coro.poll());

        let mut coro = Mutex::new(coro);
        // Trying to poll again without providing input will cause the portal to not emit an event,
        // which should cause `Coro` to panic.
        let result = catch_unwind_silent(move || coro.get_mut().unwrap().poll());
        assert!(result.is_err());
    }
}
