//! A portal implementation that supports input- and output events. See [`InOutBack`] and [`InOutFront`].

use super::input::*;
use super::output::*;
use crate::portal::{PortalBack, PortalFront};
use crate::{AsStorage, CoroPoll, Driver};
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

/// Extends [`Driver`] with a function that creates a facade.
pub trait DriverExt {
    type Output;

    /// Create a facade.
    fn facade(self) -> Self::Output;
}

impl<St, Fut> DriverExt for Driver<St>
where
    St: AsStorage<Future = Fut>,
    Fut: Future,
{
    type Output = InOutFacade<St, Fut>;

    fn facade(self) -> Self::Output {
        InOutFacade::new(self)
    }
}

/// A facade for the [`inout`](self) portal.
///
/// This facade assumes that every output event is the product of an input event. One input event may yield zero or more
/// output events.
///
/// Normal interaction with this facade looks like this:
///
/// - Check for completion with [`result()`](InOutFacade::finalize).
/// - Drive the underlying coroutine with [`drive()`](InOutFacade::drive), iterating over the output.
/// - Loop.
///
/// # Example
///
/// ```
/// use smog::driver;
/// use smog::portal::inout::{DriverExt, InOutBack};
///
/// async fn coroutine(mut portal: InOutBack<usize, u64>) -> String {
///     let mut count: u64 = 0;
///     while count < 10 {
///         let x = portal.receive().await;
///         for _ in 0..x {
///             count += 1;
///             portal.send(count).await;
///         }
///     }
///
///     format!("Got {count} points!")
/// }
///
/// # let mut input = vec![2, 5, 1, 3, 8].into_iter();
/// # let mut get_random_number = move || {
/// #     input.next().unwrap()
/// # };
/// #
/// let mut facade = driver().function(coroutine).facade();
/// while !facade.has_finished() {
///     // Drive the coroutine with a new input.
///     let input = get_random_number();
///     // The return type of `drive()` is an iterator over output events.
///     for output in facade.drive(input) {
///         println!("Input {input} yielded {output}.");
///     }
/// }
///
/// println!("{}", facade.finalize().unwrap());
/// ```
pub struct InOutFacade<St, Fut>
where
    St: AsStorage<Future = Fut>,
    Fut: Future,
{
    driver: Driver<St>,
    future_result: Option<Fut::Output>,
}

impl<St, Fut> InOutFacade<St, Fut>
where
    St: AsStorage<Future = Fut>,
    Fut: Future,
{
    /// Creates a new instance.
    pub fn new(driver: Driver<St>) -> Self {
        Self {
            driver,
            future_result: None,
        }
    }
}

impl<St, I, O, Fut> InOutFacade<St, Fut>
where
    St: AsStorage<Front = InOutFront<I, O>, Future = Fut>,
    Fut: Future,
    I: Unpin,
    O: Unpin,
{
    /// Drives the coroutine with the provided input event. The resulting iterator may generate zero or more output
    /// events.
    ///
    /// The caller should call [`has_result()`](Self::has_finished) before each call to [`drive()`](Self::drive) in order
    /// to check whether the future is completed. This is even the case if the coroutine does not return a meaningful
    /// value for the caller.
    pub fn drive(&mut self, input: I) -> impl Iterator<Item = O> + use<'_, St, I, O, Fut> {
        self.driver.portal().provide(input);
        InOutDrive { facade: self }
    }

    /// Determines whether the underlying coroutine has finished.
    pub fn has_finished(&self) -> bool {
        self.future_result.is_some()
    }

    /// Finalizes this instance and returns the result of the underlying coroutine. Whether the underlying coroutine has
    /// finished (and, by implication, a result is available) can first be checked with
    /// [`has_result()`](Self::has_finished).
    pub fn finalize(self) -> Option<Fut::Output> {
        self.future_result
    }
}

struct InOutDrive<'a, St, Fut>
where
    St: AsStorage<Future = Fut>,
    Fut: Future,
{
    facade: &'a mut InOutFacade<St, Fut>,
}

impl<St, I, O, Fut> Iterator for InOutDrive<'_, St, Fut>
where
    St: AsStorage<Front = InOutFront<I, O>, Future = Fut>,
    Fut: Future,
{
    type Item = O;

    fn next(&mut self) -> Option<Self::Item> {
        match self.facade.driver.poll() {
            CoroPoll::Event(evt) => match evt {
                InOutEvent::Awaiting => None,
                InOutEvent::Yielded(out) => Some(out),
            },
            CoroPoll::Result(res) => {
                self.facade.future_result = Some(res);
                None
            }
        }
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

    #[test]
    fn test_facade() {
        let mut facade = driver().function(create_machine).facade();

        assert!(!facade.has_finished());

        // The facade assumes that either nothing is yielded before the first await or that the caller doesn't care
        // whether the yield "belongs to" the input or is an "initial yield". Code that cares about this distinction
        // will have to use the driver directly (or create some other facade).

        assert_eq!(facade.drive(150).collect::<Vec<_>>(), vec![13, 300]);
        assert!(!facade.has_finished());

        assert_eq!(facade.drive(52).collect::<Vec<_>>(), vec![104]);
        assert!(facade.has_finished());
        assert_eq!(404, facade.finalize().expect("expected a result"));
    }
}
