//! Finding 4: a lifetime nested inside an argument type is not collected into `use<..>`.
use gtor::generator;

#[generator(yield_type = usize)]
fn g<'a>(a: Option<&'a str>) {
    if let Some(a) = a {
        yield_value!(a.len());
    }
}

fn main() {}
