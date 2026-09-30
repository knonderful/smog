//! Control: the macro handles the case it was written for (one top-level reference).
use gtor::generator;
use std::pin::pin;

#[generator(yield_type = usize)]
fn g(a: &str) {
    yield_value!(a.len());
}

fn main() {
    assert_eq!(vec![3], pin!(g("abc")).collect::<Vec<_>>());
}
