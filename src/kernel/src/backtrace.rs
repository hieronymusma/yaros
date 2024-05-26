use core::{
    fmt::{Display, Pointer},
    ptr::null,
};

use crate::{
    assert::static_assert_size,
    memory::page_tables::{XWRMode, KERNEL_PAGE_TABLES},
    println,
};

extern "C" {
    #[link_name = "llvm.frameaddress"]
    fn frame_address<'a>(level: i32) -> *const BacktraceFrame<'a>;
}

pub struct Backtrace;

impl Backtrace {
    pub fn capture() {
        println!("Backtrace:");
        // This has to happen in this function
        // We cannot put that into a separate function because othwerise the returned stack frame
        // pointer is not longer existent.
        let frame: MaybeBacktraceFrame = unsafe {
            let fp = frame_address(0);
            if fp == null() {
                None.into()
            } else {
                // Weirdly in RISC-V the previous fp is at fp[-2] and the return address at fp[-1]
                let fp = fp.sub(1);
                let kernel_page_tables = KERNEL_PAGE_TABLES.lock();
                assert!(
                    kernel_page_tables.is_active(),
                    "Kernel page tables must be active in kernel mode."
                );
                if !kernel_page_tables.is_mapped_with(fp, XWRMode::ReadWrite) {
                    println!("fp address is not mapped as readable {:p}", fp);
                    None.into()
                } else {
                    Some(&*fp).into()
                }
            }
        };
        let frame = match frame.0 {
            None => {
                println!("No valid saved fp");
                return;
            }
            Some(frame) => frame,
        };
        for (index, frame) in frame.iter().enumerate() {
            println!("{:2.}: {}", index, frame);
        }
    }
}

#[repr(transparent)]
struct MaybeBacktraceFrame<'a>(Option<&'a BacktraceFrame<'a>>);

impl<'a> From<Option<&'a BacktraceFrame<'a>>> for MaybeBacktraceFrame<'a> {
    fn from(value: Option<&'a BacktraceFrame<'a>>) -> Self {
        Self(value)
    }
}

impl<'a> Pointer for MaybeBacktraceFrame<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let ptr = match self.0 {
            Some(f) => f as *const _,
            None => null(),
        };
        write!(f, "{:p}", ptr)
    }
}

#[repr(C)]
struct BacktraceFrame<'a> {
    previous_frame: MaybeBacktraceFrame<'a>,
    previous_return_address: *const (),
}

static_assert_size!(BacktraceFrame, 16);

impl<'a> BacktraceFrame<'a> {
    fn iter(&'a self) -> BacktraceFrameIterator<'a> {
        BacktraceFrameIterator { frame: self }
    }
}

impl<'a> Display for BacktraceFrame<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:p}", self.previous_return_address)
    }
}

struct BacktraceFrameIterator<'a> {
    frame: &'a BacktraceFrame<'a>,
}

impl<'a> Iterator for BacktraceFrameIterator<'a> {
    type Item = &'a BacktraceFrame<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(frame) = self.frame.previous_frame.0 {
            let kernel_page_tables = KERNEL_PAGE_TABLES.lock();
            assert!(
                kernel_page_tables.is_active(),
                "Kernel page tables must be active in kernel mode."
            );
            if !kernel_page_tables.is_mapped_with(frame, XWRMode::ReadWrite) {
                return None;
            }
            let current = self.frame;
            self.frame = frame;
            Some(current)
        } else {
            None
        }
    }
}
