use crate::{Never, Return};
use core::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

struct MapToReturn<F> {
    future: F,
}

impl<F, R> Future for MapToReturn<F>
where
    F: Future<Output = R>,
{
    type Output = Return<R>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: We're not moving the future around in memory.
        let fut = unsafe { self.map_unchecked_mut(|this| &mut this.future) };
        fut.poll(cx).map(|r| Return(r))
    }
}

pub fn map_to_return<R>(future: impl Future<Output = R>) -> impl Future<Output = Return<R>> {
    MapToReturn { future }
}

pub fn map_to_never(future: impl Future<Output = Never>) -> impl Future<Output = Never> {
    future
}
