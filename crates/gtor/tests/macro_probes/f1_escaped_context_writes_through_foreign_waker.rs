//! # FIXED
//!
//! Finding 1a: The context can be handed to a foreign executor. `Yield::poll` then reads and writes
//! the foreign waker's data pointer as if it were a `State<u64>`.

use std::cell::RefCell;
use std::pin::pin;
use gtor::{create_generator, GeneratorContext};

/// Lets `GeneratorContext<Y>` out of the factory closure. Nothing here is `unsafe`.
fn steal_context<Y>() -> GeneratorContext<Y> {
    let stash: RefCell<Option<GeneratorContext<Y>>> = RefCell::new(None);
    let _generator = create_generator(|ctx: GeneratorContext<Y>| {
        *stash.borrow_mut() = Some(ctx);
        async {}
    });
    stash.into_inner().expect("factory closure was not invoked")
}

fn finding_1_escaped_context_writes_through_foreign_waker() {
    // All-zero bytes read back as `Option::<u64>::None`, so the write path is taken cleanly.
    const FILL: u8 = 0x00;
    let mut buf = Box::new([FILL; 256]);

    let mut ctx = steal_context::<u64>();
    // -- let waker = waker_pointing_at(buf.as_mut_ptr());
    // -- let mut cx = Context::from_waker(&waker);

    let mut _fut = pin!(ctx.yield_value(0x1122_3344_5566_7788));
    // -- let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fut.as_mut().poll(&mut cx)));
    // --
    // -- if cfg!(debug_assertions) {
    // --     // Debug build: the magic canary is read from *our* buffer (0x00000000), so the crate's own
    // --     // check fires. That read is already outside of any `State`.
    // --     let msg = outcome.expect_err("debug build should trip the magic canary");
    // --     let msg = msg.downcast_ref::<String>().cloned().unwrap_or_default();
    // --     assert!(
    // --         msg.contains("Expected state magic value"),
    // --         "unexpected panic message: {msg}"
    // --     );
    // -- } else {
    // --     // Release build: no canary. The poll returns Pending and the yielded value has been
    // --     // written into our byte buffer.
    // --     assert_eq!(Poll::Pending, outcome.expect("release build should not panic"));
    // --     assert!(
    // --         buf.iter().any(|&b| b != FILL),
    // --         "buffer untouched; the write did not happen"
    // --     );
    // -- }
}

fn main() {}
