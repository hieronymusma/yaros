use core::{
    arch::asm,
    fmt::{self, Debug},
    ops::{Deref, DerefMut},
};

#[allow(clippy::upper_case_acronyms)]
pub struct MMIO<T: Sized> {
    address: *mut T,
}

impl<T> MMIO<T> {
    pub const unsafe fn new(address: usize) -> Self {
        Self {
            address: address as *mut T,
        }
    }

    pub unsafe fn add(&self, count: usize) -> Self {
        unsafe {
            Self {
                address: self.address.add(count),
            }
        }
    }

    pub unsafe fn new_type_with_offset<U>(&self, offset: usize) -> MMIO<U> {
        MMIO::<U> {
            address: self.address.byte_add(offset) as *mut U,
        }
    }

    fn memory_barrier(&self) {
        // The Rust default is memory globber
        // Use it to force re-read of assembly
        unsafe {
            asm!("");
        }
    }

    fn memory_fence(&self) {
        // Make sure that io writes and reads are in order
        unsafe {
            asm!("fence");
        }
    }
}

impl<T> Deref for MMIO<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe {
            self.memory_barrier();
            let deref = &*self.address;
            self.memory_fence();
            deref
        }
    }
}

impl<T> DerefMut for MMIO<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            self.memory_barrier();
            let deref_mut = &mut *self.address;
            self.memory_fence();
            deref_mut
        }
    }
}

impl<T> Debug for MMIO<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        fmt::Pointer::fmt(&self.address, f)
    }
}
