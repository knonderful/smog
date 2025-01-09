# Smog

`smog` is a Rust crate that enables execution of `async` functions with interaction between the caller and the callee (often called a _coroutine_).
This enables users to write state machines that can suspend their execution in an intuitive way.
This can be useful for many cases, for example:

- Writing generators instead of delivering results in a `Vec`. ([example](examples/generator.rs))
- Writing code that can process input data in small chunks (streaming), yielding intermediate results as they become available. ([example](examples/stream_in.rs))
- Softening commitment to a blocking or an async approach by implementing the business logic in coroutines (which can be used in blocking or async).
- Writing intuitive stateful code for applications that have some sort of event loop (like a computer game).

`smog` is designed to have a reasonable amount of overhead.
Additionally, `smog` does not require heap allocation for the coroutine itself.
