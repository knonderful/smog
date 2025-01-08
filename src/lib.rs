//! A crate for working with asymmetric coroutines in pure Rust. This enables writing ergonomic
//! state machines (including generators) that allow for events to be passed into and out of the
//! coroutine, as well as evaluation of completion results.
//!
//! # Example
//!
//! ```
//! use smog::{Coro, CoroPoll};
//! use smog::portal::event::{InOutBack, InOutEvent};
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
//!     // Create the coroutine with the future that represents the work
//!     let coro = smog::coro_from_fn(|back| parse_and_find(back, "Jo"));
//!     // We need to pin the coro for technical reasons. Can be either on the heap with Box::pin() or on the stack,
//!     // like so:
//!     let mut parser = pin!(coro);
//!
//!     let mut yielded = Vec::new();
//!     let match_count;
//!     loop {
//!         // Polling "drives" the coroutine and retrieves events from the portal
//!         match parser.poll() {
//!             CoroPoll::Event(io_evt) => match io_evt {
//!                 InOutEvent::Awaiting => {
//!                     // The coroutine is waiting for input. We provide it through the portal.
//!                     parser.portal().provide(input.next().expect("Ran out of input"));
//!                 }
//!                 InOutEvent::Yielded(val) => {
//!                     // The coroutine yielded output.
//!                     yielded.push(val);
//!                 }
//!             }
//!             CoroPoll::Result(result) => match result {
//!                 // The coroutine has finished; handle the result.
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
//! implementation and the outside world a [`Portal`](portal::Portal) is established. This portal
//! consists of two sides: a front (used by the by the outside world, or "caller") and a back (used
//! by the [`Future`] implementation, or "callee"). Different [`Portal`](portal::Portal)
//! implementations provide different abilities. They dictate how the caller and callee can
//!  communicate (e.g. via input and output events).
//!
//! [`Coro`] provides an ergonomic interface for driving the coroutine state and handling events
//! that occur. The type of events that can occur depend on the [`Portal`](portal::Portal)
//! implementation.

mod cell;
pub mod portal;

use crate::portal::{PortalBack, PortalFront};
use std::future::Future;
use std::pin::Pin;
use std::ptr;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

pub struct Coro<P, F>
where
    F: Future,
{
    portal: P,
    future: F,
    waker: Waker,
    state: CoroState<F::Output>,
}

impl<P, F> Coro<P, F>
where
    F: Future,
{
    pub fn new(portal: P, future: F) -> Self {
        Self {
            portal,
            future,
            waker: unsafe { Waker::from_raw(NOOP_WAKER) },
            state: CoroState::InProgress,
        }
    }

    pub fn portal<'a, 'b>(self: &'a mut Pin<&'b mut Self>) -> &'a mut P {
        // SAFETY: We're not moving `self` in our code.
        let this = unsafe { Pin::get_unchecked_mut(self.as_mut()) };
        &mut this.portal
    }
}

pub fn coro_from_fn<B: PortalBack, F: Future>(future_fn: impl FnOnce(B) -> F) -> Coro<B::Front, F> {
    let (front, back) = B::new_portal();
    let future = future_fn(back);
    Coro::new(front, future)
}

impl<P, F> Coro<P, F>
where
    P: PortalFront,
    F: Future,
{
    #[must_use]
    pub fn poll(self: &mut Pin<&mut Self>) -> CoroPoll<P::Event, F::Output> {
        // SAFETY: We're not moving `self` in our code.
        let this = unsafe { Pin::get_unchecked_mut(self.as_mut()) };
        if let Some(event) = this.portal.poll() {
            return CoroPoll::Event(event);
        }

        match &this.state {
            CoroState::InProgress => {
                // Drive the future
                let mut poll_ctx = Context::from_waker(&this.waker);

                // SAFETY: This is OK because `self` is pinned.
                let pin = unsafe { Pin::new_unchecked(&mut this.future) };
                match pin.poll(&mut poll_ctx) {
                    Poll::Pending => {
                        // Now we're expecting at least _one_ event from the portal, since otherwise
                        // we could be running into an infinite loop here.
                        let Some(event) = this.portal.poll() else {
                            panic!("Future did not complete, but portal does not have any new events.");
                        };
                        CoroPoll::Event(event)
                    }
                    Poll::Ready(result) => {
                        // It could be that the portal still has some events. We need to process
                        // those before we signal the completion.
                        if let Some(event) = this.portal.poll() {
                            // Store the result in the Coro, it will be retrieve in a subsequent
                            // call to this function.
                            this.state = CoroState::Ready(result);
                            return CoroPoll::Event(event);
                        }

                        this.state = CoroState::Finished;
                        CoroPoll::Result(result)
                    }
                }
            }
            CoroState::Ready(_) => {
                let CoroState::Ready(result) = std::mem::replace(&mut this.state, CoroState::Finished) else {
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
fn catch_unwind_silent<F: FnOnce() -> R + std::panic::UnwindSafe, R>(f: F) -> std::thread::Result<R> {
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(f);
    std::panic::set_hook(prev_hook);
    result
}
