use core::{cell::LazyCell, fmt::Debug, ops::Deref, ptr::null_mut};

use alloc::boxed::Box;
use common::mutex::Mutex;

use crate::{
    assert::static_assert_size,
    cpu::{read_satp, write_satp_and_fence},
    debug,
    interrupts::plic,
    io::TEST_DEVICE_ADDRESSS,
    klibc::{
        elf,
        sizes::{GiB, MiB},
        util::{get_bit, get_multiple_bits, is_aligned, set_multiple_bits, set_or_clear_bit},
    },
    memory::page::PAGE_SIZE,
    processes::timer,
};

use super::{page::Page, runtime_mappings::get_runtime_mappings};

pub static KERNEL_PAGE_TABLES: LazyStaticKernelPageTables = LazyStaticKernelPageTables::new();

pub struct LazyStaticKernelPageTables {
    inner: Mutex<LazyCell<&'static RootPageTableHolder>>,
}

impl LazyStaticKernelPageTables {
    const fn new() -> Self {
        Self {
            inner: Mutex::new(LazyCell::new(|| {
                let page_tables = Box::new(RootPageTableHolder::new_with_kernel_mapping());
                Box::leak(page_tables)
            })),
        }
    }
}

impl Deref for LazyStaticKernelPageTables {
    type Target = RootPageTableHolder;

    fn deref(&self) -> &Self::Target {
        &self.inner.lock()
    }
}

// SAFETY: Inner type is wrapped within a mutex. So we can make this sync.
// Somehow it didn't worked to make this Send only. I guess some problems by using LazyCell.
unsafe impl Sync for LazyStaticKernelPageTables {}

pub struct RootPageTableHolder {
    root_table: *mut PageTable,
}

// SAFETY: PageTables can be send to another thread
unsafe impl Send for RootPageTableHolder {}

impl Debug for RootPageTableHolder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let page_table = self.table();
        write!(f, "RootPageTableHolder({:p})", page_table)
    }
}

impl Drop for RootPageTableHolder {
    fn drop(&mut self) {
        assert!(!self.is_active(), "Page table is dropped while active");
        let table = self.table();
        for first_level_entry in table.0.iter() {
            if !first_level_entry.get_validity() || first_level_entry.is_leaf() {
                continue;
            }
            let second_level_table = first_level_entry.get_target_page_table();
            for second_level_entry in second_level_table.0.iter() {
                if !second_level_entry.get_validity() || second_level_entry.is_leaf() {
                    continue;
                }
                let third_level_table = second_level_entry.get_physical_address();
                if third_level_table.is_null() {
                    continue;
                }
                let _ = unsafe { Box::from_raw(third_level_table) };
            }
            let _ = unsafe { Box::from_raw(second_level_table) };
        }
        let _ = unsafe { Box::from_raw(self.root_table) };
        self.root_table = null_mut();
    }
}

#[derive(Default)]
struct LinkerInformation {
    text_start: usize,
    text_end: usize,
    rodata_start: usize,
    rodata_end: usize,
    data_start: usize,
    data_end: usize,
    heap_start: usize,
    heap_size: usize,
}

impl LinkerInformation {
    unsafe fn new() -> Self {
        extern "C" {
            static mut TEXT_START: usize;
            static mut TEXT_END: usize;
            static mut RODATA_START: usize;
            static mut RODATA_END: usize;
            static mut DATA_START: usize;
            static mut DATA_END: usize;

            static mut HEAP_START: usize;
            static mut HEAP_SIZE: usize;
        }

        if cfg!(miri) {
            Self::default()
        } else {
            Self {
                text_start: TEXT_START,
                text_end: TEXT_END,
                rodata_start: RODATA_START,
                rodata_end: RODATA_END,
                data_start: DATA_START,
                data_end: DATA_END,
                heap_start: HEAP_START,
                heap_size: HEAP_SIZE,
            }
        }
    }

    fn text_size(&self) -> usize {
        self.text_end - self.text_start
    }

    fn rodata_size(&self) -> usize {
        self.rodata_end - self.rodata_start
    }

    fn data_size(&self) -> usize {
        self.data_end - self.data_start
    }
}

impl RootPageTableHolder {
    fn empty() -> Self {
        let root_table = Box::leak(Box::new(PageTable::zero()));
        Self { root_table }
    }

    fn table(&self) -> &PageTable {
        // SAFETY: It is always allocated
        unsafe { &*self.root_table }
    }

    fn table_mut(&mut self) -> &mut PageTable {
        // SAFETY: It is always allocated
        unsafe { &mut *self.root_table }
    }

    fn is_active(&self) -> bool {
        let satp = read_satp();
        let ppn = satp & 0xfffffffffff;
        let page_table_address = ppn << 12;

        let current_physical_address = self.table().get_physical_address();

        debug!(
            "is_active: satp: {:x}; page_table_address: {:x}",
            satp, current_physical_address
        );
        page_table_address == current_physical_address
    }

    pub fn new_with_kernel_mapping() -> Self {
        let mut root_page_table_holder = RootPageTableHolder::empty();

        let linker_information = unsafe { LinkerInformation::new() };

        root_page_table_holder.map_identity_kernel(
            linker_information.text_start,
            linker_information.text_size(),
            XWRMode::ReadExecute,
            "TEXT",
        );

        root_page_table_holder.map_identity_kernel(
            linker_information.rodata_start,
            linker_information.rodata_size(),
            XWRMode::ReadOnly,
            "RODATA",
        );

        root_page_table_holder.map_identity_kernel(
            linker_information.data_start,
            linker_information.data_size(),
            XWRMode::ReadWrite,
            "DATA",
        );

        root_page_table_holder.map_identity_kernel(
            linker_information.heap_start,
            linker_information.heap_size,
            XWRMode::ReadWrite,
            "HEAP",
        );

        root_page_table_holder.map_identity_kernel(
            plic::PLIC_BASE,
            plic::PLIC_SIZE,
            XWRMode::ReadWrite,
            "PLIC",
        );

        root_page_table_holder.map_identity_kernel(
            timer::CLINT_BASE,
            timer::CLINT_SIZE,
            XWRMode::ReadWrite,
            "CLINT",
        );

        root_page_table_holder.map_identity_kernel(
            TEST_DEVICE_ADDRESSS,
            PAGE_SIZE,
            XWRMode::ReadWrite,
            "Qemu Test Device",
        );

        for runtime_mapping in get_runtime_mappings() {
            root_page_table_holder.map_identity_kernel(
                runtime_mapping.virtual_address_start,
                runtime_mapping.size,
                runtime_mapping.privileges,
                runtime_mapping.name,
            );
        }

        root_page_table_holder
    }

    pub fn map_userspace(
        &mut self,
        virtual_address_start: usize,
        physical_address_start: usize,
        size: usize,
        privileges: XWRMode,
        name: &str,
    ) {
        self.map(
            virtual_address_start,
            physical_address_start,
            size,
            privileges,
            true,
            name,
        );
    }

    fn get_page_table_entry_for_address(&self, address: usize) -> Option<&PageTableEntry> {
        let root_page_table = self.table();

        let first_level_entry = root_page_table.get_entry_for_virtual_address(address, 2);
        if !first_level_entry.get_validity() {
            return None;
        }

        let second_level_entry = first_level_entry
            .get_target_page_table()
            .get_entry_for_virtual_address(address, 1);
        if !second_level_entry.get_validity() {
            return None;
        }

        let third_level_entry = second_level_entry
            .get_target_page_table()
            .get_entry_for_virtual_address(address, 0);
        if !third_level_entry.get_validity() {
            return None;
        }

        Some(third_level_entry)
    }

    fn map(
        &mut self,
        virtual_address_start: usize,
        physical_address_start: usize,
        size: usize,
        privileges: XWRMode,
        is_user_mode_accessible: bool,
        name: &str,
    ) {
        debug!(
            "Map \t{:#018x}-{:#018x} -> {:#018x}-{:#018x} (Size: {:#010x}) ({:?})\t({})",
            virtual_address_start,
            virtual_address_start - PAGE_SIZE + size,
            physical_address_start,
            physical_address_start - PAGE_SIZE + size,
            size,
            privileges,
            name
        );

        assert_eq!(virtual_address_start % PAGE_SIZE, 0);
        assert_eq!(physical_address_start % PAGE_SIZE, 0);
        assert_eq!(size % PAGE_SIZE, 0);

        let root_page_table = self.table_mut();

        let mut offset = 0;

        let virtual_address_with_offset = |offset| virtual_address_start + offset;
        let physical_address_with_offset = |offset| physical_address_start + offset;

        let can_be_mapped_with = |mapped_bytes, offset| {
            mapped_bytes <= (size - offset)
                && is_aligned(virtual_address_with_offset(offset), mapped_bytes)
                && is_aligned(physical_address_with_offset(offset), mapped_bytes)
        };

        // Any level of PTE can be a leaf PTE
        // So we can have 4KiB pages, 2MiB pages, and 1GiB pages in the same page table
        // They have to be aligned on 4KiB, 2MiB, and 1GiB boundaries respectively
        // We try to be smart and save memory by mapping as least as possible

        while offset < size {
            // Check if we can map a 1GiB page
            if can_be_mapped_with(GiB(1), offset) {
                let first_level_entry = root_page_table
                    .get_entry_for_virtual_address_mut(virtual_address_with_offset(offset), 2);

                assert!(
                    !first_level_entry.get_validity()
                        && first_level_entry.get_physical_address().is_null(),
                    "Entry must be an invalid value and physical address must be zero"
                );
                first_level_entry.set_xwr_mode(privileges);
                first_level_entry.set_validity(true);
                first_level_entry.set_leaf_address(physical_address_with_offset(offset));
                first_level_entry.set_user_mode_accessible(is_user_mode_accessible);
                offset += GiB(1);
                continue;
            }

            // Check if we can map a 2MiB page
            if can_be_mapped_with(MiB(2), offset) {
                let first_level_entry = root_page_table
                    .get_entry_for_virtual_address_mut(virtual_address_with_offset(offset), 2);
                if first_level_entry.get_physical_address().is_null() {
                    let page = Box::leak(Box::new(PageTable::zero()));
                    first_level_entry.set_physical_address(&mut *page);
                    first_level_entry.set_validity(true);
                }

                let second_level_entry = first_level_entry
                    .get_target_page_table()
                    .get_entry_for_virtual_address_mut(virtual_address_with_offset(offset), 1);
                assert!(
                    !second_level_entry.get_validity()
                        && second_level_entry.get_physical_address().is_null(),
                    "Entry must be an invalid value and physical address must be zero"
                );

                second_level_entry.set_xwr_mode(privileges);
                second_level_entry.set_validity(true);
                second_level_entry.set_leaf_address(physical_address_with_offset(offset));
                second_level_entry.set_user_mode_accessible(is_user_mode_accessible);
                offset += MiB(2);
                continue;
            }

            assert!(
                is_aligned(virtual_address_with_offset(offset), PAGE_SIZE),
                "Virtual address must be aligned with page size"
            );
            assert!(
                is_aligned(physical_address_with_offset(offset), PAGE_SIZE),
                "Physical address must be aligned with page size"
            );

            // Map single page
            let first_level_entry = root_page_table
                .get_entry_for_virtual_address_mut(virtual_address_with_offset(offset), 2);
            if first_level_entry.get_physical_address().is_null() {
                let page = Box::leak(Box::new(PageTable::zero()));
                first_level_entry.set_physical_address(&mut *page);
                first_level_entry.set_validity(true);
            }

            let second_level_entry = first_level_entry
                .get_target_page_table()
                .get_entry_for_virtual_address_mut(virtual_address_with_offset(offset), 1);
            if second_level_entry.get_physical_address().is_null() {
                let page = Box::leak(Box::new(PageTable::zero()));
                second_level_entry.set_physical_address(&mut *page);
                second_level_entry.set_validity(true);
            }

            let third_level_entry = second_level_entry
                .get_target_page_table()
                .get_entry_for_virtual_address_mut(virtual_address_with_offset(offset), 0);

            assert!(!third_level_entry.get_validity());

            third_level_entry.set_xwr_mode(privileges);
            third_level_entry.set_validity(true);
            third_level_entry.set_leaf_address(physical_address_with_offset(offset));
            third_level_entry.set_user_mode_accessible(is_user_mode_accessible);

            offset += PAGE_SIZE;
        }
    }

    pub fn map_identity_kernel(
        &mut self,
        virtual_address_start: usize,
        size: usize,
        privileges: XWRMode,
        name: &str,
    ) {
        self.map_identity(virtual_address_start, size, privileges, false, name);
    }

    fn map_identity(
        &mut self,
        virtual_address_start: usize,
        size: usize,
        privileges: XWRMode,
        is_user_mode_accessible: bool,
        name: &str,
    ) {
        self.map(
            virtual_address_start,
            virtual_address_start,
            size,
            privileges,
            is_user_mode_accessible,
            name,
        );
    }

    pub fn is_userspace_address(&self, address: usize) -> bool {
        self.get_page_table_entry_for_address(address)
            .map_or(false, |entry| entry.get_user_mode_accessible())
    }

    pub fn translate_userspace_address_to_physical_address<T>(
        &self,
        address: *const T,
    ) -> Option<*const T> {
        let address = address as usize;
        if !self.is_userspace_address(address) {
            return None;
        }

        let offset_from_page_start = address % PAGE_SIZE;
        self.get_page_table_entry_for_address(address).map(|entry| {
            (entry.get_physical_address() as usize + offset_from_page_start) as *const T
        })
    }
}

#[repr(C, align(4096))]
#[derive(Debug)]
struct PageTable([PageTableEntry; 512]);

static_assert_size!(PageTable, core::mem::size_of::<Page>());

impl PageTable {
    fn zero() -> Self {
        Self([PageTableEntry(null_mut()); 512])
    }

    fn get_entry_for_virtual_address_mut(
        &mut self,
        virtual_address: usize,
        level: u8,
    ) -> &mut PageTableEntry {
        assert!(level <= 2);
        let shifted_address = virtual_address >> (12 + 9 * level);
        let index = shifted_address & 0x1ff;
        &mut self.0[index]
    }

    fn get_entry_for_virtual_address(&self, virtual_address: usize, level: u8) -> &PageTableEntry {
        assert!(level <= 2);
        let shifted_address = virtual_address >> (12 + 9 * level);
        let index = shifted_address & 0x1ff;
        &self.0[index]
    }

    fn get_physical_address(&self) -> usize {
        (self as *const Self).addr()
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
struct PageTableEntry(*mut PageTable);

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XWRMode {
    PointerToNextLevel = 0b000,
    ReadOnly = 0b001,
    ReadWrite = 0b011,
    ExecuteOnly = 0b100,
    ReadExecute = 0b101,
    ReadWriteExecute = 0b111,
}

impl From<u8> for XWRMode {
    fn from(value: u8) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}

impl From<elf::ProgramHeaderFlags> for XWRMode {
    fn from(value: elf::ProgramHeaderFlags) -> Self {
        match value {
            elf::ProgramHeaderFlags::RW => Self::ReadWrite,
            elf::ProgramHeaderFlags::RWX => Self::ReadWriteExecute,
            elf::ProgramHeaderFlags::RX => Self::ReadExecute,
            elf::ProgramHeaderFlags::X => Self::ExecuteOnly,
            elf::ProgramHeaderFlags::W => panic!("Cannot map W flag"),
            elf::ProgramHeaderFlags::WX => panic!("Cannot map WX flag"),
            elf::ProgramHeaderFlags::R => Self::ReadOnly,
        }
    }
}

impl PageTableEntry {
    const VALID_BIT_POS: usize = 0;
    const READ_BIT_POS: usize = 1;
    #[allow(dead_code)]
    const WRITE_BIT_POS: usize = 2;
    #[allow(dead_code)]
    const EXECUTE_BIT_POS: usize = 3;
    const USER_MODE_ACCESSIBLE_BIT_POS: usize = 4;
    const PHYSICAL_PAGE_BIT_POS: usize = 10;
    const PHYSICAL_PAGE_BITS: usize = 0xfffffffffff;

    fn set_validity(&mut self, is_valid: bool) {
        self.0 = self.0.map_addr(|mut addr| {
            set_or_clear_bit(&mut addr, is_valid, PageTableEntry::VALID_BIT_POS)
        });
    }

    fn get_validity(&self) -> bool {
        get_bit(self.0.addr(), PageTableEntry::VALID_BIT_POS)
    }

    fn set_user_mode_accessible(&mut self, is_user_mode_accessible: bool) {
        self.0 = self.0.map_addr(|mut addr| {
            set_or_clear_bit(
                &mut addr,
                is_user_mode_accessible,
                PageTableEntry::USER_MODE_ACCESSIBLE_BIT_POS,
            )
        });
    }

    fn get_user_mode_accessible(&self) -> bool {
        get_bit(self.0.addr(), PageTableEntry::USER_MODE_ACCESSIBLE_BIT_POS)
    }

    fn set_xwr_mode(&mut self, mode: XWRMode) {
        self.0 = self.0.map_addr(|mut addr| {
            set_multiple_bits(&mut addr, mode as u8, 3, PageTableEntry::READ_BIT_POS)
        });
    }

    fn get_xwr_mode(&self) -> XWRMode {
        let bits = get_multiple_bits(self.0.addr() as u64, 3, PageTableEntry::READ_BIT_POS) as u8;
        bits.into()
    }

    fn is_leaf(&self) -> bool {
        let mode = self.get_xwr_mode();
        mode != XWRMode::PointerToNextLevel
    }

    fn set_physical_address(&mut self, address: *mut PageTable) {
        let mask: usize = !(Self::PHYSICAL_PAGE_BITS << Self::PHYSICAL_PAGE_BIT_POS);
        self.0 = address.map_addr(|new_address| {
            let mut original = self.0.addr();
            original &= mask;
            original |=
                ((new_address >> 12) & Self::PHYSICAL_PAGE_BITS) << Self::PHYSICAL_PAGE_BIT_POS;
            original
        });
    }

    fn set_leaf_address(&mut self, address: usize) {
        let mask: usize = !(Self::PHYSICAL_PAGE_BITS << Self::PHYSICAL_PAGE_BIT_POS);
        self.0 = self.0.map_addr(|_| {
            let mut original = self.0.addr();
            original &= mask;
            original |= ((address >> 12) & Self::PHYSICAL_PAGE_BITS) << Self::PHYSICAL_PAGE_BIT_POS;
            original
        });
    }

    fn get_physical_address(&self) -> *mut PageTable {
        self.0.map_addr(|addr| {
            ((addr >> Self::PHYSICAL_PAGE_BIT_POS) & Self::PHYSICAL_PAGE_BITS) << 12
        })
    }

    fn get_target_page_table(&self) -> &'static mut PageTable {
        assert!(!self.is_leaf());
        assert!(!self.get_physical_address().is_null());
        let phyiscal_address = self.get_physical_address();
        unsafe { &mut *phyiscal_address }
    }
}

pub fn activate_page_table(page_table_holder: &RootPageTableHolder) {
    let page_table_address = page_table_holder.table().get_physical_address();

    debug!(
        "Activate new page mapping (Addr of page tables 0x{:x})",
        page_table_address
    );
    let page_table_address_shifted = page_table_address >> 12;

    let satp_val = 8 << 60 | (page_table_address_shifted & 0xfffffffffff);

    unsafe {
        write_satp_and_fence(satp_val);
    };
}

#[cfg(test)]
mod tests {
    use super::RootPageTableHolder;

    #[test_case]
    fn check_drop_of_page_table_holder() {
        let mut page_table = RootPageTableHolder::empty();
        page_table.map_userspace(0x1000, 0x2000, 0x3000, super::XWRMode::ReadOnly, "Test");
    }
}
