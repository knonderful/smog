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

#[derive(Debug)]
struct Person {
    name: String,
    friends: Vec<Person>,
}

impl From<&str> for Person {
    fn from(value: &str) -> Self {
        Self {
            name: value.to_string(),
            friends: vec![],
        }
    }
}

impl Person {
    fn add_friend(&mut self, name: &str) {
        self.friends.push(name.into());
    }

    /// An example of a method with several types of lifetimes. Internally, the generator future
    /// is something like
    ///
    /// ```
    /// impl ::core::future::Future<Output=::smog::Return<&str>> + use < '_, 'a, >
    /// ```
    ///
    /// Without this Rust would complain in some cases that the function violates lifetimes of the
    /// passed arguments. Note that there are a few cases where Rust implicitly attaches lifetimes
    /// to the future, but this is not always the case.
    #[generator(yield_type = &Person)]
    fn friends<'a>(&self, prefix: &'a str) -> &str {
        for f in &self.friends {
            if f.name.starts_with(prefix) {
                yield_value!(&f);
            }
        }

        self.name.as_str()
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

    let mut hank = Person::from("Hank");
    hank.add_friend("Bob");
    hank.add_friend("Jimmy");
    hank.add_friend("Beatrice");

    let fn_name = "Person::friends()";
    for item in pin!(hank.friends("B")) {
        match item {
            GeneratorItem::Yield(friend) => println!("{fn_name} -> Yield({:?})", friend),
            GeneratorItem::Return(me) => println!("{fn_name} -> Return({:?})", me),
        }

        // hank.add_friend("Yono");
        // ^^^^
        // This wouldn't compile because the hank's lifetime is bound to the future. So as long as
        // we have a reference to the future (or to the generator that contains the future), we
        // can't modify hank.
    }
}
