//! A crate for working with asymmetric coroutines in pure Rust. This enables writing ergonomic
//! state machines (including generators) that allow for events to be passed into and out of the
//! coroutine, as well as evaluation of completion results.
//!
//! # Example
//!
//! ```
//! use smog::Coro;
//! use smog::portal::Portal;
//! use smog::portal::event::{InOutPortal, InOutBack, InOutEvent};
//! use std::pin::pin;
//!
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
//!     mut portal: InOutBack<Token, String>,
//!     prefix: &str,
//! ) -> Result<usize, String> {
//!     let Token::SectionStart(section_name) = portal.receive().await else {
//!         return Err("Expected section-start.".to_string());
//!     };
//!
//!     let mut match_count = 0;
//!     let mut current_key = None;
//!     loop {
//!         match portal.receive().await {
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
//!                     portal.send(value).await;
//!                 }
//!             }
//!         }
//!     }
//!
//!     Ok(match_count)
//! }
//!
//! fn main() {
//!     use smog::CoroPoll;
//!
//! let mut input = vec![
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
//!     let (front, back) = InOutPortal::new();
//!     let future = pin!(parse_and_find(back, "Jo"));
//!     let mut parser = Coro::new(front, future);
//!
//!     let mut yielded = Vec::new();
//!     let match_count;
//!     loop {
//!         // Polling "drives" the coroutine and retrieves events from the portal
//!         match parser.poll() {
//!             CoroPoll::Event(io_evt) => match io_evt {
//!                 InOutEvent::Awaiting => {
//!                     // The coroutine is waiting for input. We provide it through the portal.
//!                     parser.portal_mut().provide(input.next().expect("Ran out of input"));
//!                 }
//!                 InOutEvent::Yielded(val) => {
//!                     // The coroutine yielded output.
//!                     yielded.push(val);
//!                 }
//!             }
//!             CoroPoll::Result(result) => match result {
//!                 Ok(count) => {
//!                     match_count = count;
//!                     break;
//!                 }
//!                 Err(err) => panic!("Parsing failed: {err}"),
//!             }
//!         }
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
//! implementation and the outside world a [`Portal`](portal::Portal) is established. This portal consists of two
//! sides: a front (used by the by the outside world, or "caller") and a back (used by the
//! [`Future`] implementation, or "callee"). Different [`Portal`](portal::Portal) implementations
//! provide different abilities. They dictate how the caller and callee can communicate (e.g. via
//! input and output events).
//!
//! Finally, [`Coro`] unifies these concepts and provides an ergonomic interface for interacting
//! with the coroutine. Generally the calling code has the following structure in a loop:
//!
//! - Drive (advance) the coroutine and process the yielded output events.
//! - Check for completion.
//! - Provide the next input event.

pub mod portal;

use crate::portal::PortalFront;
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::ptr;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

pub struct Coro<P, F>
where
    F: Deref,
    F::Target: Future,
{
    portal: P,
    future: Pin<F>,
    waker: Waker,
    state: CoroState<<<F as Deref>::Target as Future>::Output>,
}

impl<P, F> Coro<P, F>
where
    F: DerefMut,
    F::Target: Future,
{
    pub fn new(portal: P, future: Pin<F>) -> Self {
        Self {
            portal,
            future,
            waker: unsafe { Waker::from_raw(NOOP_WAKER) },
            state: CoroState::InProgress,
        }
    }

    pub fn portal_mut(&mut self) -> &mut P {
        &mut self.portal
    }
}

impl<P, F> Coro<P, F>
where
    P: PortalFront,
    F: DerefMut,
    F::Target: Future,
{
    #[must_use]
    pub fn poll(&mut self) -> CoroPoll<P::Event, <F::Target as Future>::Output> {
        if let Some(event) = self.portal.poll() {
            return CoroPoll::Event(event);
        }

        match &self.state {
            CoroState::InProgress => {
                // Drive the future
                let mut poll_ctx = Context::from_waker(&self.waker);
                match self.future.as_mut().poll(&mut poll_ctx) {
                    Poll::Pending => {
                        // Now we're expecting at least _one_ event from the portal, since otherwise
                        // we could be running into an infinite loop here.
                        let Some(event) = self.portal.poll() else {
                            panic!(
                                "Future did not complete, but portal does not have any new events."
                            );
                        };
                        CoroPoll::Event(event)
                    }
                    Poll::Ready(result) => {
                        // It could be that the portal still has some events. We need to process
                        // those before we signal the completion.
                        if let Some(event) = self.portal.poll() {
                            // Store the result in the Coro, it will be retrieve in a subsequent
                            // call to this function.
                            self.state = CoroState::Ready(result);
                            return CoroPoll::Event(event);
                        }

                        self.state = CoroState::Finished;
                        CoroPoll::Result(result)
                    }
                }
            }
            CoroState::Ready(_) => {
                let CoroState::Ready(result) =
                    std::mem::replace(&mut self.state, CoroState::Finished)
                else {
                    unreachable!()
                };
                CoroPoll::Result(result)
            }
            CoroState::Finished => panic!("Poll called on a finished coroutine."),
        }
    }
}

/// Internal state of a [`Coro`].
enum CoroState<R> {
    InProgress,
    Ready(R),
    Finished,
}

/// Result type of [`Coro::poll()`].
#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum CoroPoll<E, R> {
    Event(E),
    Result(R),
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

#[cfg(test)]
fn catch_unwind_silent<F: FnOnce() -> R + std::panic::UnwindSafe, R>(
    f: F,
) -> std::thread::Result<R> {
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(f);
    std::panic::set_hook(prev_hook);
    result
}
