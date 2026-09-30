//! Probes for the code-review findings against `gtor`. Every probe uses only the safe, public
//! API of the crate. A probe that panics with the crate's own "BUG:" / magic-canary message, or
//! that observes a write into memory it controls, proves that safe code reached `unsafe` state
//! it was never supposed to reach.

use gtor::{create_generator, GeneratorContext, GeneratorItem};
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::{pin, Pin};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

// ---------------------------------------------------------------------------------------------
// Finding 1 (lib.rs:154): `GeneratorContext` escapes the factory closure. Once it is out, the
// `Yield` future it creates can be polled under any waker, and `Yield::poll` blindly casts
// `waker.data()` to `*mut State<Y>`.
// ---------------------------------------------------------------------------------------------

/// A waker whose data pointer points at a caller-owned byte buffer instead of a `State<Y>`.
fn waker_pointing_at(buf: *mut u8) -> Waker {
    unsafe fn clone(p: *const ()) -> RawWaker {
        RawWaker::new(p, &VTABLE)
    }
    unsafe fn noop(_: *const ()) {}
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    unsafe { Waker::from_raw(RawWaker::new(buf.cast::<()>(), &VTABLE)) }
}

/// Lets `GeneratorContext<Y>` out of the factory closure. Nothing here is `unsafe`.
fn steal_context<Y>() -> GeneratorContext<Y> {
    let stash: RefCell<Option<GeneratorContext<Y>>> = RefCell::new(None);
    let _generator = create_generator(|ctx: GeneratorContext<Y>| {
        *stash.borrow_mut() = Some(ctx);
        async {}
    });
    stash.into_inner().expect("factory closure was not invoked")
}

/// The context can be handed to a foreign executor. `Yield::poll` then reads and writes the
/// foreign waker's data pointer as if it were a `State<u64>`.
#[test]
fn finding_1_escaped_context_writes_through_foreign_waker() {
    // All-zero bytes read back as `Option::<u64>::None`, so the write path is taken cleanly.
    const FILL: u8 = 0x00;
    let mut buf = Box::new([FILL; 256]);

    let mut ctx = steal_context::<u64>();
    let waker = waker_pointing_at(buf.as_mut_ptr());
    let mut cx = Context::from_waker(&waker);

    let mut fut = pin!(ctx.yield_value(0x1122_3344_5566_7788));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fut.as_mut().poll(&mut cx)));

    if cfg!(debug_assertions) {
        // Debug build: the magic canary is read from *our* buffer (0x00000000), so the crate's own
        // check fires. That read is already outside of any `State`.
        let msg = outcome.expect_err("debug build should trip the magic canary");
        let msg = msg.downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(
            msg.contains("Expected state magic value"),
            "unexpected panic message: {msg}"
        );
    } else {
        // Release build: no canary. The poll returns Pending and the yielded value has been
        // written into our byte buffer.
        assert_eq!(Poll::Pending, outcome.expect("release build should not panic"));
        assert!(
            buf.iter().any(|&b| b != FILL),
            "buffer untouched; the write did not happen"
        );
    }
}

/// A context of one generator can be awaited inside another generator with a different yield
/// type. In a debug build the magic canary trips (the review's original repro aborts with
/// `free(): invalid pointer` in release, which cannot be caught by a test harness).
#[cfg(debug_assertions)]
#[test]
fn finding_1_escaped_context_awaited_in_other_generator() {
    let mut string_ctx = steal_context::<String>();

    let mut gen = pin!(create_generator(async move |mut ctx: GeneratorContext<u8>| {
        ctx.yield_value(1).await;
        // `String` is 24 bytes, `u8` is 1 byte: this writes an `Option<String>` into `State<u8>`.
        string_ctx.yield_value("x".repeat(100)).await;
        ctx.yield_value(2).await;
    }));

    assert_eq!(GeneratorItem::Yield(1), gen.as_mut().poll_next());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| gen.as_mut().poll_next()));
    let msg = outcome.expect_err("second poll must trip the canary or the BUG check");
    let msg = msg.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(
        msg.contains("Expected state magic value") || msg.contains("BUG:"),
        "unexpected panic message: {msg}"
    );
}

// ---------------------------------------------------------------------------------------------
// Finding 3 (lib.rs:92): `#[derive(Clone)]` on `GeneratorContext` lets two `Yield` futures be
// alive at once, which the `'_` lifetime on `yield_value` was supposed to prevent.
// ---------------------------------------------------------------------------------------------

/// Minimal `join` for two futures, so that no external crate is needed.
struct Join<A, B> {
    a: Option<Pin<Box<A>>>,
    b: Option<Pin<Box<B>>>,
}

impl<A: Future<Output = ()>, B: Future<Output = ()>> Future for Join<A, B> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if let Some(a) = self.a.as_mut() {
            if a.as_mut().poll(cx).is_ready() {
                self.a = None;
            }
        }
        if let Some(b) = self.b.as_mut() {
            if b.as_mut().poll(cx).is_ready() {
                self.b = None;
            }
        }
        if self.a.is_none() && self.b.is_none() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[test]
#[should_panic(expected = "BUG: Yield future encountered an existing value in the state.")]
fn finding_3_clone_allows_two_live_yields() {
    let mut gen = pin!(create_generator(async move |mut ctx: GeneratorContext<u32>| {
        let mut ctx2 = ctx.clone();
        Join {
            a: Some(Box::pin(ctx.yield_value(1))),
            b: Some(Box::pin(ctx2.yield_value(2))),
        }
        .await;
    }));

    let _ = gen.as_mut().poll_next();
}

/// Same defect, without any executor trickery: with `Clone` the user can simply forget to
/// `.await` one yield and start another, and the crate's `unreachable!` path is reachable.
#[test]
#[should_panic(expected = "BUG: Yield future encountered an existing value in the state.")]
fn finding_3_clone_lets_second_yield_start_before_first_finishes() {
    let mut gen = pin!(create_generator(async move |mut ctx: GeneratorContext<u32>| {
        let mut ctx2 = ctx.clone();
        let mut first = Box::pin(ctx.yield_value(1));
        // Poll the first yield once by hand (it stores its value and returns Pending), then
        // await a second one before the first has completed.
        std::future::poll_fn(|cx| {
            let _ = first.as_mut().poll(cx);
            Poll::Ready(())
        })
        .await;
        ctx2.yield_value(2).await;
    }));

    let _ = gen.as_mut().poll_next();
}

// ---------------------------------------------------------------------------------------------
// Sanity: the intended use still works, so the probes above are not "everything panics".
// ---------------------------------------------------------------------------------------------

#[test]
fn control_intended_use_works() {
    let hits = Cell::new(0);
    let mut gen = pin!(create_generator(async |mut ctx: GeneratorContext<u32>| {
        for i in 0..3 {
            hits.set(hits.get() + 1);
            ctx.yield_value(i).await;
        }
    }));
    let items: Vec<u32> = (&mut gen).collect();
    assert_eq!(vec![0, 1, 2], items);
    assert_eq!(3, hits.get());
}
