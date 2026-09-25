//! Module for creating [`Driver`] via a builder.
//!
//! Creating a [`Driver`] involves 3 steps:
//! - Step 1: Choose the [`AsStorage`] implementation (heap or stack).
//! - Step 2: Provide the [`PortalFront`] object.
//! - Step 3: Provide the [`Future`] object.
//!
//! The builder provides several methods for traversing this 3-step process:
//! - Step 1 can be skipped if heap allocation is used ([`BuilderStep1::front()`]).
//! - Step 2 can be skipped if the [`PortalBack::Front`] for the coroutine function implements [`Default`]
//!   ([`BuilderStep2::function()`]).
//! - Both step 1 and 2 can be skipped if both of the above apply
//!   ([`BuilderStep1::function()`]).
//!
//! If the [`Driver`] storage is to be allocated on the stack the user must first create a pinned [`Storage`] and pass
//! that into [`BuilderStep1::storage()`].
use crate::portal::{PortalBack, PortalFront};
use crate::{AsStorage, Driver, Storage};
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

pub struct BuilderStep1 {
    _phantom: PhantomData<()>, // forces creating this struct via `Default`.
}

pub struct BuilderStep2<St> {
    storage: St,
}

pub struct BuilderStep3<St> {
    storage: St,
}

impl Default for BuilderStep1 {
    fn default() -> Self {
        Self { _phantom: PhantomData }
    }
}

impl BuilderStep1 {
    pub fn storage<St>(self, storage: St) -> BuilderStep2<St>
    where
        St: AsStorage,
    {
        BuilderStep2 { storage }
    }

    pub fn front<Fr, Fut>(self, front: Fr) -> BuilderStep3<Pin<Box<Storage<Fr, Fut>>>>
    where
        Fr: PortalFront,
        Fut: Future,
    {
        self.storage(Box::pin(Default::default())).front(front)
    }

    #[allow(clippy::type_complexity)]
    pub fn function<Bk, Fut>(self, create_future: impl FnOnce(Bk) -> Fut) -> Driver<Pin<Box<Storage<Bk::Front, Fut>>>>
    where
        Bk: PortalBack,
        Fut: Future,
        Bk::Front: Default,
    {
        self.front(Bk::Front::default()).function(create_future)
    }
}

impl<St> BuilderStep2<St>
where
    St: AsStorage,
{
    pub fn front<Fr>(mut self, front: Fr) -> BuilderStep3<St>
    where
        St: AsStorage<Front = Fr>,
        Fr: PortalFront,
    {
        self.storage.as_storage().supply_front(front);
        BuilderStep3 { storage: self.storage }
    }

    pub fn function<Bk, Fut>(self, create_future: impl FnOnce(Bk) -> Fut) -> Driver<St>
    where
        St: AsStorage<Front = Bk::Front, Future = Fut>,
        Bk: PortalBack,
        Fut: Future,
        Bk::Front: Default,
    {
        self.front(Bk::Front::default()).function(create_future)
    }
}

impl<St> BuilderStep3<St>
where
    St: AsStorage,
{
    pub fn function<Bk, Fut>(mut self, create_future: impl FnOnce(Bk) -> Fut) -> Driver<St>
    where
        St: AsStorage<Front = Bk::Front, Future = Fut>,
        Bk: PortalBack,
        Fut: Future,
    {
        let back = Bk::new(self.storage.as_storage().pinned_front());
        self.storage.as_storage().supply_future(create_future(back));
        Driver::new(self.storage)
    }
}
