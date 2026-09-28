use smog::GeneratorItem;
use smog_macro::generator;
use std::pin::pin;

/// A finite generator that only yields values and doesn't have a final result.
///
/// The generator implements `Iterator<Item=usize>`.
#[generator(yield_type = usize)]
fn example_without_return() {
    for x in (1..10).rev() {
        if x > 5 {
            yield_value!(x);
        }
    }
}

/// A finite generator that only yields values and doesn't have a final result (explicit `-> ()`).
///
/// The generator implements `Iterator<Item=usize>`.
#[generator(yield_type = usize)]
fn example_without_return_explicit() -> () {
    for x in (1..10).rev() {
        if x > 5 {
            yield_value!(x);
        }
    }
}

/// A (finite) generator that only yields values and also has a final result.
///
/// The generator implements `Iterator<Item=GeneratorItem<usize, String>>`.
#[generator(yield_type = usize)]
fn example_with_return() -> String {
    for x in (1..10).rev() {
        if x > 5 {
            yield_value!(x);
        }
    }

    "done".to_string()
}

/// An infinite generator that only yields values and never completes.
///
/// The generator does not implement `Iterator`, since the `next() -> Option<Self::Item>` implies
/// that it will return `None` at some point. With this being an infinite generator, that time will
/// never come. Also, the caller would have unnecessarily unwrap the `Option<T>` (which the for-loop
/// mechanism in Rust does for the user, but still...).
///
/// Instead, the caller has to call `next_value()` manually. The idea being that this kind of
/// generator is used for generating IDs or something and will normally always be called on-demand
/// anyway, instead of being part of a closed loop.
#[generator(yield_type = usize)]
fn example_with_infinity() -> ! {
    let mut i = 261722;
    loop {
        yield_value!(i);
        i %= 1000000; // avoid overflow
        i = i * 2 - i / 3;
    }
}

fn main() {
    let fn_name = "example_without_return()";
    for val in pin!(example_without_return()) {
        println!("{fn_name} -> {val}");
    }

    let fn_name = "example_without_return_explicit()";
    for val in pin!(example_without_return_explicit()) {
        println!("{fn_name} -> {val}");
    }

    let fn_name = "example_with_return()";
    for val in pin!(example_with_return()) {
        match val {
            GeneratorItem::Yield(y) => println!("{fn_name} -> Yield({y})"),
            GeneratorItem::Return(r) => println!("{fn_name} -> Return({r})"),
        }
    }

    let fn_name = "example_with_infinity()";
    let mut gen = pin!(example_with_infinity());

    // If we were to use a loop here, we would get an infinite number of yields
    for _ in 0..10 {
        let val = gen.as_mut().next_value();
        println!("{fn_name} -> {val}");
    }
}
