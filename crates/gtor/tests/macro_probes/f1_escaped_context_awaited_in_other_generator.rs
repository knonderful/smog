//! # FIXED
//!
//! Finding 1b: A context of one generator can be awaited inside another generator with a different
//! yield type. In a debug build the magic canary trips (the review's original repro aborts with
//! `free(): invalid pointer` in release, which cannot be caught by a test harness).

use gtor::{create_generator, GeneratorContext};
use std::cell::RefCell;
use std::pin::pin;

/// Lets `GeneratorContext<Y>` out of the factory closure. Nothing here is `unsafe`.
fn steal_context<Y>() -> GeneratorContext<Y> {
    let stash: RefCell<Option<GeneratorContext<Y>>> = RefCell::new(None);
    let _generator = create_generator(|ctx: GeneratorContext<Y>| {
        *stash.borrow_mut() = Some(ctx);
        async {}
    });
    stash.into_inner().expect("factory closure was not invoked")
}

fn finding_1_escaped_context_awaited_in_other_generator() {
    let mut string_ctx = steal_context::<String>();

    let mut gen = pin!(create_generator(async move |mut ctx: GeneratorContext<u8>| {
        // SAFETY: `ctx` belongs to this generator
        unsafe {
            ctx.yield_value(1).await;
        }
        // `String` is 24 bytes, `u8` is 1 byte: this writes an `Option<String>` into `State<u8>`.
        // SAFETY-VIOLATION: `string_ctx` belongs to another generator
        string_ctx.yield_value("x".repeat(100)).await;
        // -- ctx.yield_value(2).await;
    }));

    // -- assert_eq!(GeneratorItem::Yield(1), gen.as_mut().poll_next());
    // -- let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| gen.as_mut().poll_next()));
    // -- let msg = outcome.expect_err("second poll must trip the canary or the BUG check");
    // -- let msg = msg.downcast_ref::<String>().cloned().unwrap_or_default();
    // -- assert!(
    // --     msg.contains("Expected state magic value") || msg.contains("BUG:"),
    // --     "unexpected panic message: {msg}"
    // -- );
}

fn main() {}
