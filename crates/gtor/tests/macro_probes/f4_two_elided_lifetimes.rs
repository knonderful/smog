//! Finding 4: two elided lifetimes collapse into one ambiguous `'_` in `use<..>`.
use gtor::generator;

#[generator(yield_type = usize)]
fn g(a: &str, b: &str) {
    yield_value!(a.len());
    yield_value!(b.len());
}

fn main() {}
