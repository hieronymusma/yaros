use core::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

/// In comparison to a plain NonNull<T> this type is send if T is send as well
/// This should be used if this is the only instance of the pointer
/// e.g. a OwningPtr represents ownership over this memory
pub struct OwningPtr<T> {
    ptr: NonNull<T>,
    phantom: PhantomData<T>,
}

unsafe impl<T: Send> Send for OwningPtr<T> {}

impl<T> OwningPtr<T> {
    pub const fn new(ptr: NonNull<T>) -> Self {
        Self {
            ptr,
            phantom: PhantomData,
        }
    }

    pub const unsafe fn new_unchecked(ptr: *mut T) -> Self {
        Self {
            ptr: NonNull::new_unchecked(ptr),
            phantom: PhantomData,
        }
    }

    pub unsafe fn cast<U>(self) -> OwningPtr<U> {
        OwningPtr {
            ptr: self.ptr.cast(),
            phantom: PhantomData,
        }
    }
}

impl<T> Deref for OwningPtr<T> {
    type Target = NonNull<T>;

    fn deref(&self) -> &Self::Target {
        &self.ptr
    }
}

impl<T> DerefMut for OwningPtr<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ptr
    }
}
