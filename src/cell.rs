#![allow(unused)]
#[cfg(not(debug_assertions))]
use optimized::{XRef, XRefCell, XRefMut};
#[cfg(debug_assertions)]
use std::cell::{Ref as XRef, RefCell as XRefCell, RefMut as XRefMut};
use std::ops::{Deref, DerefMut};

/// A [`RefCell`](std::cell::RefCell)-like container that does not do any run-time checking in
/// release mode. The idea is that we run all our tests in both debug and release mode. The debug
/// mode will detect any borrow issues, but in release mode we don't pay for the run-time checking
/// costs.
pub(crate) struct OptimizedRefCell<T>(XRefCell<T>);

impl<T> OptimizedRefCell<T> {
    /// See [`RefCell::new`](std::cell::RefCell::new).
    #[inline(always)]
    pub fn new(value: T) -> Self {
        Self(XRefCell::new(value))
    }

    /// See [`RefCell::borrow`](std::cell::RefCell::borrow).
    #[inline(always)]
    pub fn borrow(&self) -> Ref<T> {
        Ref(self.0.borrow())
    }

    /// See [`RefCell::borrow_mut`](std::cell::RefCell::borrow_mut).
    #[inline(always)]
    pub fn borrow_mut(&self) -> RefMut<T> {
        RefMut(self.0.borrow_mut())
    }

    /// See [`RefCell::replace`](std::cell::RefCell::replace).
    #[inline(always)]
    pub fn replace(&self, value: T) -> T {
        self.0.replace(value)
    }
}

pub(crate) struct Ref<'a, T>(XRef<'a, T>);

impl<T> Deref for Ref<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

pub(crate) struct RefMut<'a, T>(XRefMut<'a, T>);

impl<T> Deref for RefMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<T> DerefMut for RefMut<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

#[cfg(not(debug_assertions))]
mod optimized {
    use std::cell::UnsafeCell;
    use std::ops::{Deref, DerefMut};

    pub struct XRefCell<T> {
        value: UnsafeCell<T>,
    }

    impl<T> XRefCell<T> {
        pub fn new(value: T) -> Self {
            Self {
                value: UnsafeCell::new(value),
            }
        }

        pub fn borrow(&self) -> XRef<T> {
            XRef::new(unsafe { &*self.value.get() })
        }

        pub fn borrow_mut(&self) -> XRefMut<T> {
            XRefMut::new(unsafe { &mut *self.value.get() })
        }

        pub fn replace(&self, value: T) -> T {
            std::mem::replace(&mut *self.borrow_mut(), value)
        }
    }

    pub struct XRef<'a, T> {
        value: &'a T,
    }

    impl<'a, T> XRef<'a, T> {
        pub fn new(value: &'a T) -> Self {
            Self { value }
        }
    }

    impl<T> Deref for XRef<'_, T> {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            self.value
        }
    }

    pub struct XRefMut<'a, T> {
        value: &'a mut T,
    }

    impl<'a, T> XRefMut<'a, T> {
        pub fn new(value: &'a mut T) -> Self {
            Self { value }
        }
    }

    impl<T> Deref for XRefMut<'_, T> {
        type Target = T;

        fn deref(&self) -> &Self::Target {
            self.value
        }
    }

    impl<T> DerefMut for XRefMut<'_, T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            self.value
        }
    }
}
