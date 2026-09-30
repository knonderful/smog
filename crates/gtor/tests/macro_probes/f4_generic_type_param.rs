//! Finding 4: a generic type parameter is not mentioned in the generated `use<..>`.
use gtor::generator;

#[generator(yield_type = T)]
fn g<T: Clone>(x: T) {
    yield_value!(x.clone());
    yield_value!(x);
}

fn main() {}
