//! Probes for the code-review findings against `gtor`. Every probe uses only the safe, public
//! API of the crate. A probe that panics with the crate's own "BUG:" / magic-canary message, or
//! that observes a write into memory it controls, proves that safe code reached `unsafe` state
//! it was never supposed to reach.

use gtor::{create_generator, GeneratorContext};
use std::cell::Cell;
use std::pin::pin;

// -- NOTE: Finding 1 fixed and moved to failing compilations.

// ---------------------------------------------------------------------------------------------
// Finding 3 (lib.rs:92): `#[derive(Clone)]` on `GeneratorContext` lets two `Yield` futures be
// alive at once, which the `'_` lifetime on `yield_value` was supposed to prevent.
// ---------------------------------------------------------------------------------------------


//
// #[test]
// #[should_panic(expected = "BUG: Yield future encountered an existing value in the state.")]
// fn finding_3_clone_allows_two_live_yields() {
//     let mut gen = pin!(create_generator(async move |mut ctx: GeneratorContext<u32>| {
//         let mut ctx2 = ctx.clone();
//         // SAFETY: `ctx` and `ctx2` are called in the right scope.
//         unsafe {
//             Join {
//                 a: Some(Box::pin(ctx.yield_value(1))),
//                 b: Some(Box::pin(ctx2.yield_value(2))),
//             }
//             .await;
//         }
//     }));
//
//     let _ = gen.as_mut().poll_next();
// }
//
// /// Same defect, without any executor trickery: with `Clone` the user can simply forget to
// /// `.await` one yield and start another, and the crate's `unreachable!` path is reachable.
// #[test]
// #[should_panic(expected = "BUG: Yield future encountered an existing value in the state.")]


// ---------------------------------------------------------------------------------------------
// Sanity: the intended use still works, so the probes above are not "everything panics".
// ---------------------------------------------------------------------------------------------

#[test]
fn control_intended_use_works() {
    let hits = Cell::new(0);
    let mut gen = pin!(create_generator(async |mut ctx: GeneratorContext<u32>| {
        for i in 0..3 {
            hits.set(hits.get() + 1);
            // SAFETY: `ctx` is called in the right scope.
            unsafe {
                ctx.yield_value(i).await;
            }
        }
    }));
    let items: Vec<u32> = (&mut gen).collect();
    assert_eq!(vec![0, 1, 2], items);
    assert_eq!(3, hits.get());
}
