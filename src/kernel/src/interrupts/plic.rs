use crate::{info, klibc::MMIO};

pub const PLIC_BASE: usize = 0x0c00_0000;
pub const PLIC_SIZE: usize = 0x1000_0000;

// These constants are set to interrupt context 1 which corresponds to Supervisor Mode on Hart 0
// If we support multiple harts, we will need to change these constants to be configurable
const PRIORITY_REGISTER_BASE: MMIO<u32> = MMIO::new(PLIC_BASE);
#[allow(dead_code)]
const PENDING_REGISTER: MMIO<u32> = MMIO::new(PLIC_BASE + 0x1000);
const ENABLE_REGISTER: MMIO<u32> = MMIO::new(PLIC_BASE + 0x2080);
const THRESHOLD_REGISTER: MMIO<u32> = MMIO::new(PLIC_BASE + 0x20_1000);
const CLAIM_COMPLETE_REGISTER: MMIO<u32> = MMIO::new(PLIC_BASE + 0x20_1004);

const UART_INTERRUPT_NUMBER: u32 = 10;

#[derive(PartialEq, Eq)]
pub enum InterruptSource {
    Uart,
    Else,
}

pub fn enable(interrupt_id: u32) {
    unsafe {
        ENABLE_REGISTER.write(ENABLE_REGISTER.read() | (1 << interrupt_id));
    }
}

pub fn set_priority(interrupt_id: u32, priority: u32) {
    assert!(priority <= 7);
    unsafe {
        PRIORITY_REGISTER_BASE
            .add(interrupt_id as usize)
            .write(priority);
    }
}

pub fn set_threshold(threshold: u32) {
    assert!(threshold <= 7);
    unsafe {
        THRESHOLD_REGISTER.write(threshold);
    }
}

pub fn get_next_pending() -> Option<InterruptSource> {
    let open_interrupt = unsafe { CLAIM_COMPLETE_REGISTER.read() };

    match open_interrupt {
        0 => None,
        UART_INTERRUPT_NUMBER => Some(InterruptSource::Uart),
        _ => Some(InterruptSource::Else),
    }
}

pub fn complete_interrupt(source: InterruptSource) {
    let interrupt_id = match source {
        InterruptSource::Uart => UART_INTERRUPT_NUMBER,
        InterruptSource::Else => panic!("Invalid interrupt source to complete."),
    };
    unsafe {
        CLAIM_COMPLETE_REGISTER.write(interrupt_id);
    }
}

pub fn init_uart_interrupt() {
    info!("Initializing plic uart interrupt");
    set_threshold(0);
    enable(UART_INTERRUPT_NUMBER);
    set_priority(UART_INTERRUPT_NUMBER, 1);
}
