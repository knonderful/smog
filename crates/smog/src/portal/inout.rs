//! A portal implementation that supports input- and output events. See [`InOutBack`] and [`InOutFront`].

use super::input::*;
use super::output::*;
use crate::portal::{PortalBack, PortalFront};
use std::future::Future;
use std::pin::Pin;

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
    pub fn provide(&mut self, event: I) {
        self.input.provide(event);
    }
}

impl<I, O> PortalFront for InOutFront<I, O> {
    type Event = InOutEvent<O>;

    fn poll(mut self: Pin<&mut Self>) -> Option<Self::Event> {
        // Prioritize output over input, such that yields arrive before awaits

        // SAFETY: Returning reference as pin is safe.
        let output = unsafe { self.as_mut().map_unchecked_mut(|s| &mut s.output) };
        if let Some(event) = output.poll() {
            return match event {
                OutEvent::Yielded(value) => Some(InOutEvent::Yielded(value)),
            };
        }

        // SAFETY: Returning reference as pin is safe.
        let input = unsafe { self.map_unchecked_mut(|s| &mut s.input) };
        if let Some(event) = input.poll() {
            return match event {
                InEvent::Awaiting => Some(InOutEvent::Awaiting),
            };
        }

        None
    }
}

impl<I, O> Default for InOutFront<I, O> {
    fn default() -> Self {
        let input = InFront::default();
        let output = OutFront::default();
        Self { input, output }
    }
}

/// The [`PortalBack`] implementation.
pub struct InOutBack<I, O> {
    input: InBack<I>,
    output: OutBack<O>,
}

impl<I, O> InOutBack<I, O> {
    /// See [`OutBack::send()`].
    pub fn send(&mut self, event: O) -> impl Future<Output = ()> + use<'_, I, O> {
        self.output.send(event)
    }

    /// See [`InBack::receive()`].
    pub fn receive(&mut self) -> impl Future<Output = I> + use<'_, I, O> {
        self.input.receive()
    }
}

impl<I, O> PortalBack for InOutBack<I, O>
where
    I: Unpin,
    O: Unpin,
{
    type Front = InOutFront<I, O>;

    fn new(mut front: Pin<&mut Self::Front>) -> Self {
        let input = InBack::new(Pin::new(&mut front.input));
        let output = OutBack::new(Pin::new(&mut front.output));
        Self { input, output }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{catch_unwind_silent, driver, CoroPoll};
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
        let mut driver = driver().function(create_machine);

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
        let mut driver = driver().function(create_machine);

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
        let mut driver = driver().function(create_machine);

        // Trying to provide input without awaiting first (since the driver hasn't been polled yet)
        driver.portal().provide(150);
        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(13)), driver.poll());
        // No await here, since we provided input "early"
        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(300)), driver.poll());
        assert_eq!(CoroPoll::Event(InOutEvent::Awaiting), driver.poll());
        driver.portal().provide(52);
        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(104)), driver.poll());
        assert_eq!(CoroPoll::Result(404), driver.poll());
    }

    #[test]
    fn test_provide_without_await_2() {
        let mut driver = driver().function(create_machine);

        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(13)), driver.poll());

        // Trying to provide input without awaiting first (since the driver hasn't reached the await yet)
        driver.portal().provide(150);
        // No await here, since we provided input "early"
        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(300)), driver.poll());
        assert_eq!(CoroPoll::Event(InOutEvent::Awaiting), driver.poll());
        driver.portal().provide(52);
        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(104)), driver.poll());
        assert_eq!(CoroPoll::Result(404), driver.poll());
    }

    #[test]
    fn test_poll_after_await() {
        let storage = pin!(Default::default());
        let mut driver = driver().storage(storage).function(create_machine);

        assert_eq!(CoroPoll::Event(InOutEvent::Yielded(13)), driver.poll());
        assert_eq!(CoroPoll::Event(InOutEvent::Awaiting), driver.poll());

        let mut driver = Mutex::new(driver);
        // Trying to poll again without providing input will cause the portal to not emit an event,
        // which should cause `Driver` to panic.
        let result = catch_unwind_silent(move || driver.get_mut().unwrap().poll());
        assert!(result.is_err());
    }
}
