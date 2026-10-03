//! # FIXED
//!
//! Finding 1b: A context of one generator can be awaited inside another generator with a different
//! yield type. In a debug build the magic canary trips (the review's original repro aborts with
//! `free(): invalid pointer` in release, which cannot be caught by a test harness).

use gtor::{create_generator, GeneratorContext};
use std::future::Future;
use std::pin::pin;
use std::task::Poll;

fn finding_3_clone_lets_second_yield_start_before_first_finishes() {
    let mut gen = pin!(create_generator(async move |mut ctx: GeneratorContext<u32>| {
        let mut ctx2 = ctx.clone();
        // SAFETY: `ctx` is called in the right scope.
        let mut first = unsafe { Box::pin(ctx.yield_value(1)) };
        // Poll the first yield once by hand (it stores its value and returns Pending), then
        // await a second one before the first has completed.
        std::future::poll_fn(|cx| {
            let _ = first.as_mut().poll(cx);
            Poll::Ready(())
        })
        .await;
        // SAFETY: `ctx2` is called in the right scope.
        unsafe {
            ctx2.yield_value(2).await;
        }
    }));

    let _ = gen.as_mut().poll_next();
}

fn main() {}
