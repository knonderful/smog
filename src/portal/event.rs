use std::future::Future;

mod input;
mod output;

use crate::portal::{PortalBack, PortalFront};
pub use input::*;
pub use output::*;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum InOutEvent<O> {
    /// The back side is awaiting an event.
    Awaiting,
    /// The back side has yielded an event.
    Yielded(O),
}

pub struct InOutFront<I, O> {
    input: InFront<I>,
    output: OutFront<O>,
}

impl<I, O> InOutFront<I, O> {
    fn new(input: InFront<I>, output: OutFront<O>) -> Self {
        Self { input, output }
    }

    pub fn provide(&mut self, event: I) {
        self.input.provide(event);
    }
}

impl<I, O> PortalFront for InOutFront<I, O> {
    type Event = InOutEvent<O>;

    fn poll(&mut self) -> Option<Self::Event> {
        // Prioritize output over input, such that yields arrive before awaits
        if let Some(event) = self.output.poll() {
            return match event {
                OutEvent::Yielded(value) => Some(InOutEvent::Yielded(value)),
            };
        }

        if let Some(event) = self.input.poll() {
            return match event {
                InEvent::Awaiting => Some(InOutEvent::Awaiting),
            };
        }

        None
    }
}

pub struct InOutBack<I, O> {
    input: InBack<I>,
    output: OutBack<O>,
}

impl<I, O> InOutBack<I, O> {
    fn new(input: InBack<I>, output: OutBack<O>) -> Self {
        Self { input, output }
    }

    /// See [`OutBack::send()`].
    pub fn send(&mut self, event: O) -> impl Future<Output = ()> + use<'_, I, O> {
        self.output.send(event)
    }

    /// See [`InBack::receive()`].
    pub fn receive(&mut self) -> impl Future<Output = I> + use<'_, I, O> {
        self.input.receive()
    }
}

impl<I, O> PortalBack for InOutBack<I, O> {
    type Front = InOutFront<I, O>;

    fn new_portal() -> (Self::Front, Self) {
        let (in_front, in_back) = InBack::new_portal();
        let (out_front, out_back) = OutBack::new_portal();

        (Self::Front::new(in_front, out_front), Self::new(in_back, out_back))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{catch_unwind_silent, coro_from_fn, CoroPoll};
    use std::pin::pin;
    use std::sync::Mutex;

    async fn create_machine(mut portal: InOutBack<u32, u64>) -> u64 {
        const ROOT_VALUE: u64 = 13;
        portal.send(ROOT_VALUE).await;

        let mut values = Vec::new();
        while values.len() < 2 {
            let value = u64::from(portal.receive().await) * 2;
            portal.send(value).await;
            values.push(value);
        }

        values.iter().sum()
    }

    #[test]
    fn test_valid() {
        let mut coro = pin!(coro_from_fn(create_machine));

        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(13)), coro.poll());
        assert_eq!(CoroPoll::Event(InOutEvent::Awaiting), coro.poll());
        coro.portal().provide(150);
        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(300)), coro.poll());
        assert_eq!(CoroPoll::Event(InOutEvent::Awaiting), coro.poll());
        coro.portal().provide(52);
        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(104)), coro.poll());
        assert_eq!(CoroPoll::Result(404), coro.poll());
    }

    #[test]
    fn test_poll_after_completion() {
        let mut coro = pin!(coro_from_fn(create_machine));

        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(13)), coro.poll());
        assert_eq!(CoroPoll::Event(InOutEvent::Awaiting), coro.poll());
        coro.portal().provide(150);
        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(300)), coro.poll());
        assert_eq!(CoroPoll::Event(InOutEvent::Awaiting), coro.poll());
        coro.portal().provide(52);
        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(104)), coro.poll());
        assert_eq!(CoroPoll::Result(404), coro.poll());

        let mut coro = Mutex::new(coro);
        // Trying to poll after completion should panic.
        let result = catch_unwind_silent(move || coro.get_mut().unwrap().poll());
        assert!(result.is_err());
    }

    #[test]
    fn test_provide_without_await_1() {
        let coro = pin!(coro_from_fn(create_machine));
        let mut coro = Mutex::new(coro);

        // Trying to provide input without awaiting first (since the coro hasn't been polled yet)
        let result = catch_unwind_silent(move || coro.get_mut().unwrap().portal().provide(150));
        assert!(result.is_err());
    }

    #[test]
    fn test_provide_without_await_2() {
        let mut coro = pin!(coro_from_fn(create_machine));

        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(13)), coro.poll());

        let mut coro = Mutex::new(coro);

        // Trying to provide input without awaiting first (since the coro hasn't reached the await yet)
        let result = catch_unwind_silent(move || coro.get_mut().unwrap().portal().provide(150));
        assert!(result.is_err());
    }

    #[test]
    fn test_poll_after_await() {
        let mut coro = pin!(coro_from_fn(create_machine));

        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(13)), coro.poll());
        assert_eq!(CoroPoll::Event(InOutEvent::Awaiting), coro.poll());

        let mut coro = Mutex::new(coro);
        // Trying to poll again without providing input will cause the portal to not emit an event,
        // which should cause `Coro` to panic.
        let result = catch_unwind_silent(move || coro.get_mut().unwrap().poll());
        assert!(result.is_err());
    }
}
