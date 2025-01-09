//! A portal implementation that supports input- and output events. See [`InOutBack`] and [`InOutFront`].

use std::future::Future;

pub use super::input::*;
pub use super::output::*;
use crate::portal::{PortalBack, PortalFront};

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum InOutEvent<O> {
    /// The back side is awaiting an event.
    Awaiting,
    /// The back side has yielded an event.
    Yielded(O),
}

/// The [`PortalFront`] implementation.
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

/// The [`PortalBack`] implementation.
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
    use crate::{catch_unwind_silent, create_driver, CoroPoll};
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
        let mut driver = pin!(create_driver(create_machine));

        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(13)), driver.poll());
        assert_eq!(CoroPoll::Event(InOutEvent::Awaiting), driver.poll());
        driver.portal().provide(150);
        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(300)), driver.poll());
        assert_eq!(CoroPoll::Event(InOutEvent::Awaiting), driver.poll());
        driver.portal().provide(52);
        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(104)), driver.poll());
        assert_eq!(CoroPoll::Result(404), driver.poll());
    }

    #[test]
    fn test_poll_after_completion() {
        let mut driver = pin!(create_driver(create_machine));

        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(13)), driver.poll());
        assert_eq!(CoroPoll::Event(InOutEvent::Awaiting), driver.poll());
        driver.portal().provide(150);
        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(300)), driver.poll());
        assert_eq!(CoroPoll::Event(InOutEvent::Awaiting), driver.poll());
        driver.portal().provide(52);
        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(104)), driver.poll());
        assert_eq!(CoroPoll::Result(404), driver.poll());

        let mut driver = Mutex::new(driver);
        // Trying to poll after completion should panic.
        let result = catch_unwind_silent(move || driver.get_mut().unwrap().poll());
        assert!(result.is_err());
    }

    #[test]
    fn test_provide_without_await_1() {
        let driver = pin!(create_driver(create_machine));
        let mut driver = Mutex::new(driver);

        // Trying to provide input without awaiting first (since the driver hasn't been polled yet)
        let result = catch_unwind_silent(move || driver.get_mut().unwrap().portal().provide(150));
        assert!(result.is_err());
    }

    #[test]
    fn test_provide_without_await_2() {
        let mut driver = pin!(create_driver(create_machine));

        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(13)), driver.poll());

        let mut driver = Mutex::new(driver);

        // Trying to provide input without awaiting first (since the driver hasn't reached the await yet)
        let result = catch_unwind_silent(move || driver.get_mut().unwrap().portal().provide(150));
        assert!(result.is_err());
    }

    #[test]
    fn test_poll_after_await() {
        let mut driver = pin!(create_driver(create_machine));

        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(13)), driver.poll());
        assert_eq!(CoroPoll::Event(InOutEvent::Awaiting), driver.poll());

        let mut driver = Mutex::new(driver);
        // Trying to poll again without providing input will cause the portal to not emit an event,
        // which should cause `Driver` to panic.
        let result = catch_unwind_silent(move || driver.get_mut().unwrap().poll());
        assert!(result.is_err());
    }
}
