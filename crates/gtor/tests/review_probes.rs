//! Probes for the code-review findings against `gtor`. Every probe uses only the safe, public
//! API of the crate. A probe that panics with the crate's own "BUG:" / magic-canary message, or
//! that observes a write into memory it controls, proves that safe code reached `unsafe` state
//! it was never supposed to reach.

use gtor::{create_generator, GeneratorContext};
use std::cell::Cell;
use std::pin::pin;

// -- NOTE: Finding 1 fixed and moved to failing compilations.
// -- NOTE: Finding 3 fixed and moved to failing compilations.

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
