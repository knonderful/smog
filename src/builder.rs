#![allow(clippy::type_complexity)]
use crate::portal::{PortalBack, PortalFront};
use crate::{AsStorage, Driver, Storage};
use std::future::Future;
use std::pin::Pin;

pub struct BuilderStep1;
pub struct BuilderStep2<St> {
    storage: St,
}

pub struct BuilderStep3<St> {
    storage: St,
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
        self.storage(Box::pin(Storage::new2())).front(front)
    }

    pub fn coro<Bk, Fut>(self, create_future: impl FnOnce(Bk) -> Fut) -> Driver<Pin<Box<Storage<Bk::Front, Fut>>>>
    where
        Bk: PortalBack,
        Fut: Future,
        Bk::Front: Default,
    {
        self.front(Bk::Front::default()).coro(create_future)
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

    pub fn coro<Bk, Fut>(self, create_future: impl FnOnce(Bk) -> Fut) -> Driver<St>
    where
        St: AsStorage<Front = Bk::Front, Future = Fut>,
        Bk: PortalBack,
        Fut: Future,
        Bk::Front: Default,
    {
        self.front(Bk::Front::default()).coro(create_future)
    }
}

impl<St> BuilderStep3<St>
where
    St: AsStorage,
{
    pub fn coro<Bk, Fut>(mut self, create_future: impl FnOnce(Bk) -> Fut) -> Driver<St>
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
