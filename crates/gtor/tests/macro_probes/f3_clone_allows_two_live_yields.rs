//! # FIXED
//!
//! Finding 1b: A context of one generator can be awaited inside another generator with a different
//! yield type. In a debug build the magic canary trips (the review's original repro aborts with
//! `free(): invalid pointer` in release, which cannot be caught by a test harness).

use gtor::{create_generator, GeneratorContext};
use std::future::Future;
use std::pin::{pin, Pin};
use std::task::{Context, Poll};

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

fn finding_3_clone_allows_two_live_yields() {
    let mut gen = pin!(create_generator(async move |mut ctx: GeneratorContext<u32>| {
        let mut ctx2 = ctx.clone();
        // SAFETY: `ctx` and `ctx2` are called in the right scope.
        unsafe {
            Join {
                a: Some(Box::pin(ctx.yield_value(1))),
                b: Some(Box::pin(ctx2.yield_value(2))),
            }
            .await;
        }
    }));

    let _ = gen.as_mut().poll_next();
}

fn main() {}
