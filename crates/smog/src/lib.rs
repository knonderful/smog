//! A crate for writing generators using async Rust.
#[cfg(test)]
mod test;

use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

const STATE_MAGIC: u32 = 0x32561810;

struct State<Y> {
    magic_value: u32,
    yielded: Option<Y>,
}

impl<Y> Default for State<Y> {
    fn default() -> Self {
        Self {
            magic_value: STATE_MAGIC,
            yielded: None,
        }
    }
}

#[derive(Clone)]
pub struct Yielder<Y> {
    phantom_data: PhantomData<fn() -> Y>,
}

impl<Y> Yielder<Y> {
    fn new() -> Self {
        Self {
            phantom_data: PhantomData,
        }
    }

    pub fn yeeld(&mut self, value: Y) -> impl Future<Output = ()> + '_ {
        Yield::new(value)
    }
}

#[must_use]
struct Yield<'a, Y> {
    value: Option<Y>,
    phantom_data: PhantomData<&'a ()>,
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
        // SAFETY: We're adhering to pin constraints here.
        let this = unsafe { self.get_unchecked_mut() };
        match this.value.take() {
            None => Poll::Ready(()),
            Some(value) => {
                // SAFETY: This depends on the the correct pointer being set in the executor (in
                //         this case the `Generator::next()` implementation. Since `Yield` can not
                //         be constructed outside of this module, we should be good. The only thing
                //         that can happen is if `Yield` is returned from the generator, in which
                //         case it would escape its context. Followed by a call to poll with another
                //         executor would cause trouble here.
                let state = unsafe {
                    match ctx.waker().data().cast::<State<Y>>().cast_mut().as_mut() {
                        None => {
                            panic!("The waker data pointer is not set. Are you running this future in an external executor?");
                        }
                        Some(x) => x,
                    }
                };

                if state.magic_value != STATE_MAGIC {
                    panic!(
                        "Expected state magic value {STATE_MAGIC}, but found {}",
                        state.magic_value
                    );
                }

                if state.yielded.replace(value).is_some() {
                    panic!("Yield future encountered an existing value in the state.");
                }

                Poll::Pending
            }
        }
    }
}

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

pub enum GeneratorItem<Y, R> {
    Yield(Y),
    Return(R),
}

pub trait GeneratorOutput<T> {
    type Item;

    fn create_yield(value: T) -> Self::Item;
    fn create_return(result: Self) -> Option<Self::Item>;
}

impl<T> GeneratorOutput<T> for () {
    type Item = T;

    fn create_yield(value: T) -> Self::Item {
        value
    }

    fn create_return(_result: Self) -> Option<Self::Item> {
        None
    }
}

impl<T, A, B> GeneratorOutput<T> for Result<A, B> {
    type Item = GeneratorItem<T, Result<A, B>>;

    fn create_yield(value: T) -> Self::Item {
        GeneratorItem::Yield(value)
    }

    fn create_return(result: Result<A, B>) -> Option<Self::Item> {
        Some(GeneratorItem::Return(result))
    }
}

impl<F, Y> Generator<F, Y>
where
    F: Future,
    F::Output: GeneratorOutput<Y>,
{
    pub fn next(self: Pin<&mut Self>) -> Option<<F::Output as GeneratorOutput<Y>>::Item> {
        if self.finished {
            return None;
        }

        // We're putting a pointer to the state on the waker. This pointer will be used by `Yield`
        // to set the yielded value directly in the state. Since `self` is pinned here, this is OK.
        // Also, the future (`Yield`) is only
        let waker = generator_waker(&self.state);
        let mut cx = Context::from_waker(&waker);

        unsafe {
            let this = self.get_unchecked_mut();
            match Pin::new_unchecked(&mut this.future).poll(&mut cx) {
                Poll::Pending => {
                    if let Some(value) = this.state.yielded.take() {
                        return Some(F::Output::create_yield(value));
                    }
                    panic!(
                        "Underlying task is pending, but we have no yielded value. This means the generator implementation is awaiting an unsupported type of future."
                    );
                }
                Poll::Ready(output) => {
                    this.finished = true;
                    F::Output::create_return(output)
                }
            }
        }
    }
}

// NB: We can't implement Iterator for Pin, which is why we need this wrapper.
struct GeneratorIter<X> {
    pinned: Pin<X>,
}

// NB: We can't implement IntoIterator for Pin, which is why we need this wrapper.
pub trait GeneratorIntoIter<X> {
    type Item;
    fn into_iter(self) -> impl Iterator<Item = Self::Item>;
}

impl<F, Y, X> GeneratorIntoIter<X> for Pin<X>
where
    F: Future,
    F::Output: GeneratorOutput<Y>,
    X: Deref<Target = Generator<F, Y>> + DerefMut,
{
    type Item = <F::Output as GeneratorOutput<Y>>::Item;

    fn into_iter(self) -> impl Iterator<Item = <F::Output as GeneratorOutput<Y>>::Item> {
        GeneratorIter { pinned: self }
    }
}

impl<F, Y, X> Iterator for GeneratorIter<X>
where
    F: Future,
    F::Output: GeneratorOutput<Y>,
    X: Deref<Target = Generator<F, Y>> + DerefMut,
{
    type Item = <F::Output as GeneratorOutput<Y>>::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.pinned.as_mut().next()
    }
}

impl<F, Y, X> From<X> for Generator<F, Y>
where
    X: FnOnce(Yielder<Y>) -> F,
{
    fn from(future_factory: X) -> Self {
        let state = State::default();
        let future = future_factory(Yielder::new());
        Generator::new(future, state)
    }
}

fn generator_waker<Y>(state: &State<Y>) -> Waker {
    unsafe fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VTABLE)
    }

    unsafe fn wake(_: *const ()) {}

    unsafe fn wake_by_ref(_: *const ()) {}

    unsafe fn drop(_: *const ()) {}

    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);

    let state_ptr = state as *const State<Y>;
    unsafe { Waker::from_raw(RawWaker::new(state_ptr.cast::<()>(), &VTABLE)) }
}
