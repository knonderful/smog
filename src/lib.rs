//! A crate for working with asymmetric coroutines in pure Rust. This enables writing ergonomic state machines
//! (including generators) that allow for events to be passed into and out of the coroutine, as well as evaluation of
//! completion results.
//!
//! # Example
//!
//! ```
//! use smog::portal::inout::{InOutBack, InOutEvent};
//! use smog::CoroPoll;
//!
//! /// Fictional output of a tokenizer.
//! #[derive(Debug)]
//! enum Token {
//!     SectionStart,
//!     SectionEnd,
//!     Key(String),
//!     Value(String),
//! }
//!
//! /// Simple state machine that parses a "section" and yields all values for which the key starts
//! /// with `prefix`.
//! async fn parse_and_find(mut portal: InOutBack<Token, String>, prefix: &str) -> Result<&'static str, String> {
//!     // Receive input from the outside world through the portal
//!     match portal.receive().await {
//!         Token::SectionStart => {}
//!         other => return Err(format!("Expected SectionStart, but got {other:?}.")),
//!     };
//!
//!     let mut match_count = 0;
//!     loop {
//!         let key = match portal.receive().await {
//!             Token::SectionEnd => break,
//!             Token::Key(val) => val,
//!             other => return Err(format!("Expected Key, but got {other:?}.")),
//!         };
//!
//!         let value = match portal.receive().await {
//!             Token::Value(val) => val,
//!             other => return Err(format!("Expected Value, but got {other:?}.")),
//!         };
//!
//!         if key.starts_with(prefix) {
//!             match_count += 1;
//!             // Send output through the portal
//!             portal.send(value).await;
//!         }
//!     }
//!
//!     const DAYS: &[&str] = &["Monday", "Tuesday", "Wednesday", "Thursday", "Friday"];
//!     Ok(DAYS[match_count % DAYS.len()])
//! }
//!
//! fn main() {
//!     let mut input = vec![
//! #        Token::SectionStart,
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
//!     let mut parser = smog::driver(|back| parse_and_find(back, "Jo"));
//!
//!     let mut yielded = Vec::new();
//!     let day;
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
//!             },
//!             CoroPoll::Result(result) => match result {
//!                 // The coroutine has finished; handle the result.
//!                 Ok(val) => {
//!                     day = val;
//!                     break;
//!                 }
//!                 Err(err) => panic!("Parsing failed: {err}"),
//!             },
//!         }
//!     }
//!
//!     assert_eq!(&yielded, &["Doe", "Hallenbeck"]);
//!     assert_eq!("Wednesday", day);
//! }
//! ```
//!
//! Here, `parse_and_find()` implements the state machine for parsing a "section" of a fictional tokenized input. The
//! code can be read from top to bottom and the reader can easily reason about the workings of this parser.
//!
//! See the Design section for an elaboration of the mechanisms involved.
//!
//! # Design
//!
//! This crate leverages the ability of the Rust compiler to generate a state machine from an `async` function. This
//! state machine is basically an inline [`Future`] implementation. Such a [`Future`] can be polled by hand, thus
//! "driving" the state machine forward.
//!
//! In order to support input and output events (data, messages, ...) between the [`Future`] implementation and the
//! outside world a [portal] is established. The portal implementation dictates how the calling code and coroutine can
//! communicate (e.g. via input- and output events).
//!
//! [`Driver`] provides an ergonomic interface for driving the coroutine state and handling events that occur. The
//! [`Storage`] contains the state of the coroutine. It contains the [`PortalFront`] object as well as the [`Future`].
//! The user can decide whether this data should be allocated on the stack or on the heap. A [`Driver`] can only be
//! created on a [pinned](std::pin) [`Storage`]. The reason for this is twofold:
//!
//! - [`Future`] objects must be pinned in order to do anything meaningful with them.
//! - The [`PortalFront`] holds the state for the portal. This state is likely shared via some sort of reference or
//!   pointer from [`PortalBack`] into the [`PortalFront`]. This can only be done safely if the [`PortalFront`] is
//!   pinned.
//!
//! Note that the user may still move the [`Driver`] around; it is only the [`Storage`] that must be pinned.

mod cell;
pub mod portal;

use crate::portal::{PortalBack, PortalFront};
use std::future::Future;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::ptr;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

/// Create a new [`Driver`] from a function that implements a coroutine. The storage is allocated on the heap.
///
/// See [crate documentation](crate) for more information.
#[allow(clippy::type_complexity)]
pub fn driver<Bk, Fut>(create_future: impl FnOnce(Bk) -> Fut) -> Driver<Pin<Box<Storage<Bk::Front, Fut>>>>
where
    Bk: PortalBack,
    Fut: Future,
    Bk::Front: Default,
{
    let storage = Box::pin(Storage::new(Bk::Front::default()));
    driver_with_storage(storage, create_future)
}

/// Create a new [`Driver`] from a function that implements a coroutine with the provided [`Storage`].
/// This is useful for constructing a driver that is allocated on the stack.
///
/// See [crate documentation](crate) for more information.
pub fn driver_with_storage<St, Fut, Bk>(storage: St, create_future: impl FnOnce(Bk) -> Fut) -> Driver<St>
where
    St: AsStorage<Front = Bk::Front, Future = Fut>,
    Fut: Future,
    Bk: PortalBack,
{
    new_driver(storage, create_future)
}

pub struct Storage<Fr, Fut> {
    front: Fr,
    future: MaybeUninit<Fut>,
    supplied_future: bool,
}

impl<Fr, Fut> Default for Storage<Fr, Fut>
where
    Fr: Default,
{
    fn default() -> Self {
        Self::new(Fr::default())
    }
}

impl<Fr, Fut> Drop for Storage<Fr, Fut> {
    fn drop(&mut self) {
        if self.supplied_future {
            unsafe {
                self.future.assume_init_drop();
            }
        }
    }
}

impl<Fr, Fut> Storage<Fr, Fut> {
    fn new(front: Fr) -> Self {
        Self {
            front,
            future: MaybeUninit::uninit(),
            supplied_future: false,
        }
    }
}

impl<Fr, Fut> Storage<Fr, Fut> {
    fn pinned_front(self: Pin<&mut Self>) -> Pin<&mut Fr> {
        let this = unsafe { self.get_unchecked_mut() };
        unsafe { Pin::new_unchecked(&mut this.front) }
    }

    fn pinned_future(self: Pin<&mut Self>) -> Pin<&mut Fut> {
        debug_assert!(self.supplied_future, "Future not yet supplied");

        let this = unsafe { self.get_unchecked_mut() };
        unsafe { Pin::new_unchecked(this.future.assume_init_mut()) }
    }

    fn supply_future(self: Pin<&mut Self>, future: Fut) {
        debug_assert!(!self.supplied_future, "Future already supplied");

        let this = unsafe { self.get_unchecked_mut() };
        this.future = MaybeUninit::new(future);
        this.supplied_future = true;
    }
}

/// Trait for retrieving a pinned [`Storage`] from a reference.
pub trait AsStorage {
    type Front: PortalFront;
    type Future: Future;

    fn as_pin(&mut self) -> Pin<&mut Storage<Self::Front, Self::Future>>;
}

impl<Fr, Fut> AsStorage for Pin<Box<Storage<Fr, Fut>>>
where
    Fr: PortalFront,
    Fut: Future,
{
    type Front = Fr;
    type Future = Fut;

    fn as_pin(&mut self) -> Pin<&mut Storage<Self::Front, Self::Future>> {
        self.as_mut()
    }
}

impl<Fr, Fut> AsStorage for Pin<&mut Storage<Fr, Fut>>
where
    Fr: PortalFront,
    Fut: Future,
{
    type Front = Fr;
    type Future = Fut;

    fn as_pin(&mut self) -> Pin<&mut Storage<Self::Front, Self::Future>> {
        self.as_mut()
    }
}

/// A coroutine driver. See the [crate documentation](crate) for more information.
pub struct Driver<St>
where
    St: AsStorage,
{
    storage: St,
    waker: Waker,
    state: CoroState<<<St as AsStorage>::Future as Future>::Output>,
}

fn new_driver<St, Bk, Fut>(mut storage: St, create_future: impl FnOnce(Bk) -> Fut) -> Driver<St>
where
    St: AsStorage<Front = Bk::Front, Future = Fut>,
    Bk: PortalBack,
    Fut: Future,
{
    let back = Bk::new(storage.as_pin().pinned_front());
    storage.as_pin().supply_future(create_future(back));

    Driver {
        storage,
        waker: unsafe { Waker::from_raw(CANARY_WAKER) },
        state: CoroState::InProgress,
    }
}

impl<St> Driver<St>
where
    St: AsStorage,
{
    /// Retrieves a reference to the [`PortalFront`].
    pub fn portal(&mut self) -> Pin<&mut St::Front> {
        self.storage.as_pin().pinned_front()
    }

    /// Polls the [`Driver`] for events. Internally, calling this function advances the underlying future and iteracts
    /// with the portal.
    #[must_use]
    pub fn poll(
        &mut self,
    ) -> CoroPoll<<<St as AsStorage>::Front as PortalFront>::Event, <<St as AsStorage>::Future as Future>::Output> {
        let mut storage = self.storage.as_pin();
        if let Some(event) = storage.as_mut().pinned_front().poll() {
            return CoroPoll::Event(event);
        }

        match &self.state {
            CoroState::InProgress => {
                // Drive the future
                let mut poll_ctx = Context::from_waker(&self.waker);

                // SAFETY: This is OK because `self` is pinned.
                match storage.as_mut().pinned_future().poll(&mut poll_ctx) {
                    Poll::Pending => {
                        // Now we're expecting at least _one_ event from the portal, since otherwise
                        // we could be running into an infinite loop here.
                        let Some(event) = storage.as_mut().pinned_front().poll() else {
                            panic!("Future did not complete, but portal does not have any new events.");
                        };
                        CoroPoll::Event(event)
                    }
                    Poll::Ready(result) => {
                        // It could be that the portal still has some events. We need to process
                        // those before we signal the completion.
                        if let Some(event) = storage.as_mut().pinned_front().poll() {
                            // Store the result in the Driver, it will be retrieve in a subsequent
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
                let CoroState::Ready(result) = std::mem::replace(&mut self.state, CoroState::Finished) else {
                    unreachable!()
                };
                CoroPoll::Result(result)
            }
            CoroState::Finished => panic!("Poll called on a finished coroutine."),
        }
    }
}

/// Internal state of a [`Driver`].
enum CoroState<Res> {
    InProgress,
    Ready(Res),
    Finished,
}

/// Result type of [`Driver::poll()`].
#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum CoroPoll<Evt, Res> {
    /// A portal event.
    Event(Evt),
    /// The result from the [`Future`].
    Result(Res),
}

const CANARY_WAKER: RawWaker = {
    const VTABLE: RawWakerVTable = RawWakerVTable::new(
        // We currently don't support futures to actually use the waker mechanism inside coroutines.
        |_| panic!("Future attempted to clone a waker. This is not supported (yet)."),
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

#[cfg(test)]
mod test {
    use super::*;
    use crate::portal::input::InBack;
    use std::any::Any;
    use std::pin::pin;
    use std::sync::Mutex;
    use std::thread::sleep;

    fn expect_panic<T>(result: Result<T, Box<dyn Any + Send>>, expected_msg: &str) {
        let Err(error) = result else {
            panic!("Expected a panic.");
        };
        let Some(msg) = error.as_ref().downcast_ref::<&str>() else {
            panic!("Expected a &str from panic.");
        };
        assert_eq!(*msg, expected_msg);
    }

    #[test]
    fn test_infinite_loop_future() {
        struct InfiniteLoopFuture;

        impl Future for InfiniteLoopFuture {
            type Output = ();

            fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
                Poll::Pending
            }
        }

        let coro = |_: InBack<u32>| async {
            InfiniteLoopFuture.await;
        };

        let driver = pin!(driver(coro));
        let mut driver = Mutex::new(driver);

        let result = catch_unwind_silent(move || driver.get_mut().unwrap().poll());
        expect_panic(
            result,
            "Future did not complete, but portal does not have any new events.",
        );
    }

    /// Tests that "foreign futures" (ones that work in an _actual_ async run-time) cause a panic, rather than leading
    /// to an infinite loop. The plan is to support waking in the future via some kind of blocking mutex mechanism in
    /// Driver::poll().
    #[test]
    fn test_foreign_future() {
        #[derive(Default)]
        struct ForeignFuture;

        impl Future for ForeignFuture {
            type Output = ();

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                // This is technically a correct future, but we don't support waking yet...
                let waker = cx.waker().clone();
                // ... so the test will panic on the clone and never reach this place.
                std::thread::spawn(|| {
                    sleep(std::time::Duration::from_secs(1));
                    waker.wake();
                });
                Poll::Pending
            }
        }

        let coro = |_: InBack<u32>| async {
            // This will grab a waker
            ForeignFuture.await;
        };

        let driver = pin!(driver(coro));
        let mut driver = Mutex::new(driver);

        let result = catch_unwind_silent(move || driver.get_mut().unwrap().poll());
        expect_panic(
            result,
            "Future attempted to clone a waker. This is not supported (yet).",
        );
    }
}
