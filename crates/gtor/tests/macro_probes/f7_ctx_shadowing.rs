//! Finding 7: a user binding named `ctx` shadows the macro's hidden context.
use gtor::generator;

#[generator(yield_type = usize)]
fn g() {
    let ctx = 5usize;
    yield_value!(ctx);
}

fn main() {}
