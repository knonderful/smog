//! A crate for writing generators using async Rust.
#[cfg(test)]
mod test;

mod never;
pub use never::Never;

pub mod future;

use std::marker::PhantomData;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

/// Magic number embedded in the [`State`]. Used as a canary in debug builds only.
const STATE_MAGIC: u32 = 0x32561810;

/// The generator state.
struct State<Y> {
    #[cfg(debug_assertions)]
    magic_value: u32,
    yielded: Option<Y>,
}

impl<Y> State<Y> {
    /// Asserts the correctness of the embedded magic number.
    #[cfg(debug_assertions)]
    fn assert_magic_number(&self) {
        if self.magic_value != STATE_MAGIC {
            panic!(
                "Expected state magic value {STATE_MAGIC}, but found {}",
                self.magic_value
            );
        }
    }

    #[cfg(not(debug_assertions))]
    fn assert_magic_number(&self) {
        // Do nothing
    }
}

impl<Y> Default for State<Y> {
    fn default() -> Self {
        Self {
            #[cfg(debug_assertions)]
            magic_value: STATE_MAGIC,
            yielded: None,
        }
    }
}

/// The context inside a generator function.
///
/// This can be used to yield a value to the caller.
#[derive(Clone)]
pub struct GeneratorContext<Y> {
    phantom_data: PhantomData<fn() -> Y>,
}

impl<Y> GeneratorContext<Y> {
    fn new() -> Self {
        Self {
            phantom_data: PhantomData,
        }
    }

    /// Yields a value from the generator to the caller.
    ///
    /// Be sure to call `.await` on the resulting [`Future`].
    #[must_use]
    pub fn yield_value(&mut self, value: Y) -> impl Future<Output = ()> + '_ {
        Yield::new(value)
    }
}

/// A yield operation, containing the yielded value.
///
/// `Yield` contains a lifetime reference to the [`GeneratorContext`] that spawned it, preventing
///  multiple yields from existing at the same time (which would be a bug in the generator
/// function). Additionally, the lifetime reference prevents a `Yield` from escaping the generator
/// function.
struct Yield<'a, Y> {
    value: Option<Y>,
    phantom_data: PhantomData<&'a mut ()>,
}

impl<Y> Yield<'_, Y> {
    fn new(value: Y) -> Self {
        Self {
            value: Some(value),
            phantom_data: PhantomData,
        }
    }
}

impl<Y> Future for Yield<'_, Y> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<()> {
        // SAFETY: We're not moving `this` or any of its components around in memory.
        let this = unsafe { self.get_unchecked_mut() };
        match this.value.take() {
            None => Poll::Ready(()),
            Some(value) => {
                // SAFETY:
                // This depends on the the correct pointer being set in the executor (in this case
                // the `Generator::next()` implementation. `Yield` nor `GeneratorContext` can not be
                // constructed by the user. This means that a `Yield` can only appear inside of a
                // `Generator`.
                //
                // Secondly, `Yield` can not escape its  encapsulating `Future` (async function)
                // because it is impossible to declare the correct return type:
                // - `GeneratorContext::yield_value()` does not name the concrete type.
                // - `Yield` is constructed with a lifetime tied to the `GeneratorContext` inside
                //    the `Future`. It is therefor impossible to specify a declare lifetime for the
                //    `Future<Output=Yield<'a, ...>>`.
                let state = unsafe {
                    match ctx.waker().data().cast::<State<Y>>().cast_mut().as_mut() {
                        None => unreachable!("BUG: The waker data pointer is not set."),
                        Some(x) => x,
                    }
                };

                // A bug-detection mechanism. It is only enabled in debug build.
                state.assert_magic_number();

                if state.yielded.replace(value).is_some() {
                    unreachable!("BUG: Yield future encountered an existing value in the state.");
                }

                Poll::Pending
            }
        }
    }
}

/// A [`Future`]-based generator that can yield any number of instances of `Y`.
pub struct Generator<F, Y> {
    future: F,
    state: State<Y>,
    finished: bool,
}

impl<F, Y> Generator<F, Y> {
    fn new(future: F, state: State<Y>) -> Self {
        Self {
            future,
            state,
            finished: false,
        }
    }
}

/// An iterator item spawned from a generator that returns a result value.
///
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum GeneratorItem<Y, R> {
    Yield(Y),
    Return(R),
}

#[must_use]
pub struct Return<R>(R);

impl<R> From<R> for Return<R> {
    fn from(value: R) -> Self {
        Self(value)
    }
}

pub trait IterableGenerator<Y> {
    type Item;

    fn create_yield(value: Y) -> Self::Item;
    fn create_return(result: Self) -> Option<Self::Item>;
}

impl<Y> IterableGenerator<Y> for () {
    type Item = Y;

    fn create_yield(value: Y) -> Self::Item {
        value
    }

    fn create_return(_result: Self) -> Option<Self::Item> {
        None
    }
}

impl<Y, R> IterableGenerator<Y> for Return<R> {
    type Item = GeneratorItem<Y, R>;

    fn create_yield(value: Y) -> Self::Item {
        GeneratorItem::Yield(value)
    }

    fn create_return(result: Self) -> Option<Self::Item> {
        Some(GeneratorItem::Return(result.0))
    }
}

impl<F, Y, R> Generator<F, Y>
where
    F: Future<Output = R>,
{
    pub fn poll_next(self: Pin<&mut Self>) -> GeneratorItem<Y, R> {
        debug_assert!(!self.finished, "Future was polled after it has finished.");

        // We're putting a pointer to the state on the waker. This pointer will be used by `Yield`
        // to set the yielded value directly in the state. This would be OK even if `self` were not
        // pinned here, since the pointer is only accessed inside of the `Future::poll()` below and
        // we're not moving the generator around in memory during that time.
        let waker = generator_waker(&self.state);
        let mut cx = Context::from_waker(&waker);

        unsafe {
            // SAFETY:
            // - The future is immediately pinned again.
            // - The state is not moved in memory in this method.
            let this = self.get_unchecked_mut();
            match Pin::new_unchecked(&mut this.future).poll(&mut cx) {
                Poll::Pending => {
                    if let Some(value) = this.state.yielded.take() {
                        return GeneratorItem::Yield(value);
                    }

                    // We can't prevent future implementations from awaiting a foreign future (i.e.
                    // a future that does not belong to this crate) at compile-time. But we have to
                    // take into account that the user steps into this trap and detect such cases.
                    panic!(
                        "Underlying task is pending, but we have no yielded value. This means the generator implementation is awaiting an unsupported type of future."
                    );
                }
                Poll::Ready(value) => {
                    this.finished = true;
                    GeneratorItem::Return(value)
                }
            }
        }
    }
}

impl<F, Y> Generator<F, Y>
where
    F: Future,
    F::Output: IterableGenerator<Y>,
{
    fn iter_next(mut self: Pin<&mut Self>) -> Option<<F::Output as IterableGenerator<Y>>::Item> {
        if self.finished {
            return None;
        }

        match self.as_mut().poll_next() {
            GeneratorItem::Yield(yielded) => Some(F::Output::create_yield(yielded)),
            GeneratorItem::Return(result) => {
                // SAFETY: Nothing is moved in memory here.
                F::Output::create_return(result)
            }
        }
    }
}

impl<F, Y> Generator<F, Y>
where
    F: Future<Output = Never>,
{
    pub fn next_value(self: Pin<&mut Self>) -> Y {
        match self.poll_next() {
            GeneratorItem::Yield(yielded) => yielded,
            GeneratorItem::Return(_) => panic!("BUG: Future with return type Never somehow managed to complete."),
        }
    }
}

impl<F, Y, R> Generator<F, Y>
where
    F: Future<Output = Return<R>>,
{
    pub fn next_item(self: Pin<&mut Self>) -> GeneratorItem<Y, R> {
        match self.poll_next() {
            GeneratorItem::Yield(yielded) => GeneratorItem::Yield(yielded),
            GeneratorItem::Return(Return(result)) => GeneratorItem::Return(result),
        }
    }
}

impl<'a, F, Y> Iterator for Pin<&'a mut Generator<F, Y>>
where
    F: Future,
    F::Output: IterableGenerator<Y>,
{
    type Item = <F::Output as IterableGenerator<Y>>::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.as_mut().iter_next()
    }
}

impl<F, Y> Iterator for Pin<Box<Generator<F, Y>>>
where
    F: Future,
    F::Output: IterableGenerator<Y>,
{
    type Item = <F::Output as IterableGenerator<Y>>::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.as_mut().iter_next()
    }
}

fn generator_waker<Y>(state: &State<Y>) -> Waker {
    unsafe fn clone(data_ptr: *const ()) -> RawWaker {
        RawWaker::new(data_ptr, &VTABLE)
    }
    unsafe fn wake(_: *const ()) {}
    unsafe fn wake_by_ref(_: *const ()) {}
    unsafe fn drop(_: *const ()) {}

    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);

    let state_ptr = state as *const State<Y>;
    unsafe { Waker::from_raw(RawWaker::new(state_ptr.cast::<()>(), &VTABLE)) }
}

pub fn generator<F, Y>(future_factory: impl FnOnce(GeneratorContext<Y>) -> F) -> Generator<F, Y>
where
    F: Future,
{
    let state = State::default();
    let future = future_factory(GeneratorContext::new());
    Generator::new(future, state)
}

pub fn generator_mapped<F, Y, F2>(
    future_factory: impl FnOnce(GeneratorContext<Y>) -> F,
    future_map: impl FnOnce(F) -> F2,
) -> Generator<F2, Y>
where
    F: Future,
{
    let state = State::default();
    let future = future_factory(GeneratorContext::new());
    Generator::new(future_map(future), state)
}
