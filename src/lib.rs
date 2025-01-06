//! A crate for working with asymmetric coroutines in pure Rust. This enables writing ergonomic
//! state machines (including generators) that allow for events to be passed into and out of the
//! coroutine, as well as evaluation of completion results.
//!
//! # Example
//!
//! ```
//! # use smog::*;
//! /// Fictional output of a tokenizer.
//! enum Token {
//!     SectionStart(String),
//!     SectionEnd,
//!     Key(String),
//!     Value(String),
//! }
//!
//! /// Simple state machine that parses a "section" and yields all values for which the key starts
//! /// with `prefix`.
//! async fn parse_and_find(
//!     mut portal: PortalCoro<Token, String>,
//!     prefix: &str,
//! ) -> Result<usize, String> {
//!     let Token::SectionStart(section_name) = coro_poll!(portal) else {
//!         return Err("Expected section-start.".to_string());
//!     };
//!
//!     let mut match_count = 0;
//!     let mut current_key = None;
//!     loop {
//!         match coro_poll!(portal) {
//!             Token::SectionStart(new_name) => {
//!                 return Err(format!("Multiple sections: {section_name} and {new_name}."))
//!             }
//!             Token::SectionEnd => break,
//!             Token::Key(key) => {
//!                 let last_key = current_key.replace(key);
//!                 if last_key.is_some() {
//!                     return Err("Multiple keys.".to_string());
//!                 }
//!             }
//!             Token::Value(value) => {
//!                 let Some(key) = current_key.take() else {
//!                     return Err("Got value without key.".to_string());
//!                 };
//!
//!                 if key.starts_with(prefix) {
//!                     match_count += 1;
//!                     coro_yield!(portal, value);
//!                 }
//!             }
//!         }
//!     }
//!
//!     Ok(match_count)
//! }
//!
//! fn main() {
//!     let mut input = vec![
//! #        Token::SectionStart("users".to_string()),
//! #        Token::Key("Bob".to_string()),
//! #        Token::Value("Bumble".to_string()),
//! #        Token::Key("John".to_string()),
//! #        Token::Value("Doe".to_string()),
//! #        Token::Key("Missy".to_string()),
//! #        Token::Value("Moe".to_string()),
//! #        Token::Key("Joseph".to_string()),
//! #        Token::Value("Hallenbeck".to_string()),
//! #        Token::SectionEnd,
//!         /* ... */
//!     ]
//!     .into_iter();
//!
//!     let (portal_coro, portal_world) = portal();
//!     let mut parser = Coro::new(portal_world, Box::pin(parse_and_find(portal_coro, "Jo")));
//!
//!     let mut yielded = Vec::new();
//!     let match_count;
//!     loop {
//!         // Drive the coroutine
//!         for out in parser.driver() {
//!             yielded.push(out);
//!         }
//!
//!         // Check for completion
//!         if let Some(result) = parser.result() {
//!             match result {
//!                 Ok(count) => {
//!                     match_count = count;
//!                     break;
//!                 }
//!                 Err(err) => panic!("Parsing failed: {err}"),
//!             }
//!         }
//!
//!         // Provide input for next iteration
//!         parser.provide(input.next().expect("Ran out of input"));
//!     }
//!
//!     assert_eq!(&yielded, &["Doe", "Hallenbeck"]);
//!     assert_eq!(2, match_count);
//! }
//! ```
//!
//! Here, `parse_and_find()` implements the state machine for parsing a "section" of a fictional
//! tokenized input. The code can be read from top to bottom and the reader can easily reason about
//! the workings of this parser.
//!
//! See the Design section for an elaboration of the mechanisms involved.
//!
//! # Design
//!
//! This crate leverages the ability of the Rust compiler to generate a state machine from an
//! `async` function. This state machine is basically an inline [`Future`] implementation. Such a
//! [`Future`] can be polled by hand, thus "driving" the state machine forward.
//!
//! In order to support input and output events (data, messages, ...) between the [`Future`]
//! implementation and the outside world a "portal" is established. This portal consists of two
//! sides: [`PortalCoro`] (used by the [`Future`] implementation) and [`PortalWorld`] (used by the
//! by the outside world). The [`coro_poll`] and [`coro_yield`] macros should be used for input and
//! output, respectively.
//!
//! Finally, [`Coro`] unifies these concepts and provides an ergonomic interface for interacting
//! with the coroutine. Generally the calling code has the following structure in a loop:
//!
//! - Drive (advance) the coroutine and process the yielded output events.
//! - Check for completion.
//! - Provide the next input event.

use std::cell::RefCell;
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::ptr;
use std::rc::Rc;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

/// Creates a portal between the inside and the outside of a coroutine. This function sets up the
/// portal and returns the two endpoints: one for the coroutine and one for the "world" outside of
/// the coroutine.
pub fn portal<I, O>() -> (PortalCoro<I, O>, PortalWorld<I, O>) {
    let portal = Rc::new(RefCell::new(PortalState::Neutral));
    (PortalCoro::new(portal.clone()), PortalWorld::new(portal))
}

#[macro_export]
macro_rules! coro_poll {
    ($ctx:ident) => {
        $ctx.poll().await
    };
}

#[macro_export]
macro_rules! coro_yield {
    ($ctx:ident, $val:expr) => {
        $ctx.do_yield($val).await
    };
}

pub struct Coro<I, O, F>
where
    F: Deref,
    F::Target: Future,
{
    future: Pin<F>,
    portal: PortalWorld<I, O>,
    waker: Waker,
    state: CoroState<<<F as Deref>::Target as Future>::Output>,
}

impl<I, O, F> Coro<I, O, F>
where
    F: DerefMut,
    F::Target: Future,
{
    pub fn new(portal: PortalWorld<I, O>, future: Pin<F>) -> Self {
        Self {
            future,
            portal,
            waker: unsafe { Waker::from_raw(NOOP_WAKER) },
            state: CoroState::InProgress,
        }
    }

    pub fn driver(&mut self) -> CoroDriver<'_, I, O, F> {
        debug_assert!(matches!(self.state, CoroState::InProgress));
        CoroDriver::new(self)
    }

    pub fn provide(&mut self, input: I) {
        debug_assert!(matches!(self.state, CoroState::InProgress));
        self.portal.provide(input);
    }

    pub fn result(&mut self) -> Option<<<F as Deref>::Target as Future>::Output> {
        match self.state {
            CoroState::InProgress => None,
            CoroState::Ready(_) => {
                let CoroState::Ready(result) =
                    std::mem::replace(&mut self.state, CoroState::Finished)
                else {
                    unreachable!()
                };
                Some(result)
            }
            CoroState::Finished => None,
        }
    }

    fn advance(&mut self) -> Option<O> {
        if !matches!(self.state, CoroState::InProgress) {
            return None;
        }

        let mut poll_ctx = Context::from_waker(&self.waker);
        // Manually drive the future
        match self.future.as_mut().poll(&mut poll_ctx) {
            Poll::Pending => self.portal.consume(),
            Poll::Ready(result) => {
                self.state = CoroState::Ready(result);
                None
            }
        }
    }
}

const NOOP_WAKER: RawWaker = {
    const VTABLE: RawWakerVTable = RawWakerVTable::new(
        // Cloning just returns a new no-op raw waker
        |_| NOOP_WAKER,
        // `wake` does nothing
        |_| {},
        // `wake_by_ref` does nothing
        |_| {},
        // Dropping does nothing as we don't allocate anything
        |_| {},
    );
    RawWaker::new(ptr::null(), &VTABLE)
};

/// Internal state of a [`Coro`].
enum CoroState<R> {
    InProgress,
    Ready(R),
    Finished,
}

/// The driver used for advancing the [`Coro`] and for receiving coroutine output.
pub struct CoroDriver<'a, I, O, F>
where
    F: Deref,
    F::Target: Future,
{
    coro: &'a mut Coro<I, O, F>,
}

impl<'a, I, O, F> Iterator for CoroDriver<'a, I, O, F>
where
    F: DerefMut,
    F::Target: Future,
{
    type Item = O;

    fn next(&mut self) -> Option<Self::Item> {
        self.coro.advance()
    }
}

impl<'a, I, O, F> CoroDriver<'a, I, O, F>
where
    F: Deref,
    F::Target: Future,
{
    fn new(coro: &'a mut Coro<I, O, F>) -> Self {
        Self { coro }
    }
}

enum PortalState<I, O> {
    /// Neutral state. This is the initial state as well as the state after a future has been
    /// resolved. From this state it is possible to transition into `IncomingAwaiting` or
    /// `OutgoingYielded`.
    Neutral,
    /// Awaiting an incoming event.
    IncomingAwaiting,
    /// An incoming event has been provided.
    IncomingProvided(I),
    /// An outgoing event is available.
    OutgoingYielded(O),
    /// The outgoing event has been consumed.
    OutgoingConsumed,
    /// The portal is closed. This happens when either side of the portal is dropped.
    Closed,
}

/// The coroutine side of the portal. See [`portal()`].
pub struct PortalCoro<I, O> {
    state: Rc<RefCell<PortalState<I, O>>>,
}

impl<I, O> Drop for PortalCoro<I, O> {
    fn drop(&mut self) {
        let _ = self.state.replace(PortalState::Closed);
    }
}

impl<I, O> PortalCoro<I, O> {
    fn new(state: Rc<RefCell<PortalState<I, O>>>) -> Self {
        Self { state }
    }

    pub fn poll(&mut self) -> impl Future<Output = I> + use<'_, I, O> {
        PollFuture::new(self.state.deref())
    }

    pub fn do_yield(&mut self, event: O) -> impl Future<Output = ()> + use<'_, I, O> {
        YieldFuture::new(self.state.deref(), event)
    }
}

struct PollFuture<'a, I, O> {
    state: &'a RefCell<PortalState<I, O>>,
}

impl<'a, I, O> PollFuture<'a, I, O> {
    fn new(state: &'a RefCell<PortalState<I, O>>) -> Self {
        let old_state = state.replace(PortalState::IncomingAwaiting);
        match old_state {
            PortalState::Neutral => {} // fall-through
            PortalState::IncomingAwaiting => panic!("Polling while state is IncomingAwaiting"),
            PortalState::IncomingProvided(_) => panic!("Polling while state is IncomingProvided"),
            PortalState::OutgoingYielded(_) => panic!("Polling while state is OutgoingYielded"),
            PortalState::OutgoingConsumed => panic!("Polling while state is OutgoingConsumed"),
            PortalState::Closed => panic!("Polling while state is Closed"),
        }

        PollFuture { state }
    }
}

impl<I, O> Future for PollFuture<'_, I, O> {
    type Output = I;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let old_state = self.state.replace(PortalState::IncomingAwaiting);

        match old_state {
            PortalState::IncomingAwaiting => Poll::Pending,
            PortalState::IncomingProvided(value) => {
                let _ = self.state.replace(PortalState::Neutral);
                Poll::Ready(value)
            }
            PortalState::Neutral => panic!("Polling while state is Neutral"),
            PortalState::OutgoingYielded(_) => panic!("Polling while state is OutgoingYielded"),
            PortalState::OutgoingConsumed => panic!("Polling while state is OutgoingConsumed"),
            PortalState::Closed => panic!("Polling while state is Closed"),
        }
    }
}

struct YieldFuture<'a, I, O> {
    state: &'a RefCell<PortalState<I, O>>,
}

impl<'a, I, O> YieldFuture<'a, I, O> {
    fn new(state: &'a RefCell<PortalState<I, O>>, event: O) -> Self {
        let old_state = state.replace(PortalState::OutgoingYielded(event));
        match old_state {
            PortalState::Neutral => {}
            PortalState::IncomingAwaiting => panic!("Yielding while state is IncomingAwaiting"),
            PortalState::IncomingProvided(_) => panic!("Yielding while state is IncomingProvided"),
            PortalState::OutgoingYielded(_) => panic!("Yielding while state is OutgoingYielded"),
            PortalState::OutgoingConsumed => panic!("Yielding while state is OutgoingConsumed"),
            PortalState::Closed => panic!("Yielding while state is Closed"),
        }

        YieldFuture { state }
    }
}

impl<I, O> Future for YieldFuture<'_, I, O> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match *self.state.borrow() {
            PortalState::OutgoingYielded(_) => return Poll::Pending,
            PortalState::OutgoingConsumed => {} // fall-through
            PortalState::Neutral => panic!("Polling while state is Neutral"),
            PortalState::IncomingAwaiting => panic!("Polling while state is IncomingAwaiting"),
            PortalState::IncomingProvided(_) => panic!("Polling while state is IncomingProvided"),
            PortalState::Closed => panic!("Polling while state is Closed"),
        }
        let _ = self.state.replace(PortalState::Neutral);
        Poll::Ready(())
    }
}

/// The "world" side of the portal. See [`portal()`].
pub struct PortalWorld<I, O> {
    state: Rc<RefCell<PortalState<I, O>>>,
}

impl<I, O> Drop for PortalWorld<I, O> {
    fn drop(&mut self) {
        let _ = self.state.replace(PortalState::Closed);
    }
}

impl<I, O> PortalWorld<I, O> {
    fn new(state: Rc<RefCell<PortalState<I, O>>>) -> Self {
        Self { state }
    }

    fn provide(&mut self, event: I) {
        let old_state = self.state.replace(PortalState::IncomingProvided(event));
        match old_state {
            PortalState::IncomingAwaiting => {}
            PortalState::Neutral => {}
            PortalState::IncomingProvided(_) => {
                panic!("Providing input while state is IncomingProvided")
            }
            PortalState::OutgoingYielded(_) => {
                panic!("Providing input while state is OutgoingYielded")
            }
            PortalState::OutgoingConsumed => {
                panic!("Providing input while state is OutgoingConsumed")
            }
            PortalState::Closed => panic!("Providing input while state is Closed"),
        }
    }

    fn consume(&mut self) -> Option<O> {
        match *self.state.borrow() {
            PortalState::Neutral => return None,
            PortalState::IncomingAwaiting => return None,
            PortalState::IncomingProvided(_) => {
                panic!("Consuming output while state is IncomingProvided")
            }
            PortalState::OutgoingYielded(_) => {} // fall-through
            PortalState::OutgoingConsumed => {
                panic!("Consuming output while state is OutgoingConsumed")
            }
            PortalState::Closed => panic!("Consuming output while state is Closed"),
        }

        let PortalState::OutgoingYielded(event) = self.state.replace(PortalState::OutgoingConsumed)
        else {
            unreachable!()
        };
        Some(event)
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use std::pin::pin;
    use std::sync::Mutex;

    fn catch_unwind_silent<F: FnOnce() -> R + std::panic::UnwindSafe, R>(
        f: F,
    ) -> std::thread::Result<R> {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = std::panic::catch_unwind(f);
        std::panic::set_hook(prev_hook);
        result
    }

    async fn case_1(mut portal: PortalCoro<u32, u64>) -> u64 {
        coro_yield!(portal, 5550123);
        let mut values = Vec::new();
        while values.len() < 2 {
            let value = u64::from(coro_poll!(portal)) * 2;
            values.push(value);
            for v in &values {
                coro_yield!(portal, *v);
            }
        }

        values.iter().sum()
    }

    fn verify_case_1<F>(mut coro: Coro<u32, u64, F>)
    where
        F: DerefMut,
        F::Target: Future<Output = u64>,
    {
        // Before first poll
        let mut driver = coro.driver();
        assert_eq!(Some(5550123), driver.next());
        assert_eq!(None, driver.next());
        assert_eq!(None, coro.result());

        // Try to yield without providing anything
        let mut driver = coro.driver();
        assert_eq!(None, driver.next());

        coro.provide(12);
        let mut driver = coro.driver();
        assert_eq!(Some(24), driver.next());
        assert_eq!(None, driver.next());

        // Try to yield without providing anything (again)
        let mut driver = coro.driver();
        assert_eq!(None, driver.next());

        coro.provide(34);
        let mut driver = coro.driver();
        assert_eq!(Some(24), driver.next());
        assert_eq!(Some(68), driver.next());
        assert_eq!(None, driver.next());

        // Provided 2 values: should be done now
        assert_eq!(Some(92), coro.result());
    }

    #[test]
    fn test_case_1_stack() {
        let (portal_coro, portal_world) = portal();
        let fut = pin!(case_1(portal_coro));
        verify_case_1(Coro::new(portal_world, fut));
    }

    #[test]
    fn test_case_1_heap() {
        let (portal_coro, portal_world) = portal();
        verify_case_1(Coro::new(portal_world, Box::pin(case_1(portal_coro))));
    }

    #[test]
    fn test_case_1_provide_too_early() {
        let (portal_coro, portal_world) = portal();
        // We need a mutex to be able to satisfy UnwindSafe for catch_unwind()
        let mut coro = Mutex::new(Coro::new(portal_world, Box::pin(case_1(portal_coro))));

        // This sets up the coroutine in an invalid state, since the implementation yields before it
        // polls for the first time.
        coro.get_mut().unwrap().provide(12);
        // Now driving the coroutine should panic:
        let result = catch_unwind_silent(move || coro.get_mut().unwrap().driver().next());
        assert!(result.is_err());
    }
}
