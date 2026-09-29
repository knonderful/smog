# Gtor

`gtor` allows users to write generators in Rust that are easy to write and easy to poll.

```rust
use gtor::{GeneratorItem, generator};
use std::pin::pin;

#[generator(yield_type = usize)]
fn example_without_return() {
  for x in (1..10).rev() {
    if x > 5 {
      yield_value!(x);
    }
  }
}

fn main() {
  let mut expected = [9, 8, 7, 6].into_iter();
  for val in pin!(example_without_return()) {
    assert_eq!(expected.next().unwrap(), val);
  }
  assert!(expected.next().is_none());
}
```

Some highlights about the generators:

* They can yield values.
* They can take input arguments.
* They can optionally return a result upon completion.
* All input arguments, yields and return values can be passed by value or by reference.
  * The borrow checker prevents any borrowing violations, just like in regular Rust code.
* They are stackless (i.e. they don't have their own separate stack that is suspended and restored).
* They can live on the stack (using `pin!()`) or the heap (using `Box::pin()`).
