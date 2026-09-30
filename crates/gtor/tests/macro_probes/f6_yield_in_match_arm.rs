//! Finding 6: `yield_value!` in expression position is not rewritten.
use gtor::generator;

#[generator(yield_type = usize)]
fn g(a: Option<usize>) {
    match a {
        Some(x) => yield_value!(x),
        None => {}
    }
}

fn main() {}
