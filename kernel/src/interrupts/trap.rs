use super::trap_cause::{InterruptCause, exception::ENVIRONMENT_CALL_FROM_U_MODE, interrupt::*};
use crate::{
    cpu::{self, read_satp, write_satp_and_fence},
    debug,
    interrupts::plic::{self, InterruptSource},
    io::{stdin_buf::STDIN_BUFFER, uart},
    memory::{
        linker_information::LinkerInformation,
        page_tables::{KERNEL_PAGE_TABLES, activate_page_table},
    },
    processes::scheduler::{self, get_current_process},
    syscalls::handle_syscall,
    warn,
};
use common::syscalls::trap_frame::{Register, TrapFrame};
use core::panic;

#[unsafe(no_mangle)]
extern "C" fn supervisor_mode_trap(
    cause: InterruptCause,
    stval: usize,
    sepc: usize,
    trap_frame: &mut TrapFrame,
) {
    let old_tables = read_satp();
    debug!("Activate KERNEL_PAGE_TABLES");
    activate_page_table(&KERNEL_PAGE_TABLES);
    debug!(
        "Supervisor mode trap occurred! (sepc: {:x?}) (cause: {:?})",
        sepc,
        cause.get_reason(),
    );
    if cause.is_interrupt() {
        handle_interrupt(cause, stval, sepc, trap_frame);
    } else {
        handle_exception(cause, stval, sepc, trap_frame);
    }

    // Restore old page tables
    // SAFTEY: They should be valid. If a process dies we don't come here
    // because the scheduler returns with restore_user_context
    // Hoewever: This is very ugly and prone to error.
    // TODO: Find a better way to do this
    unsafe {
        write_satp_and_fence(old_tables);
    }
    debug!("Return from supervisor_mode_trap");
}

fn handle_exception(cause: InterruptCause, stval: usize, sepc: usize, trap_frame: &mut TrapFrame) {
    match cause.get_exception_code() {
        ENVIRONMENT_CALL_FROM_U_MODE => {
            let nr = trap_frame[Register::a0];
            let arg1 = trap_frame[Register::a1];
            let arg2 = trap_frame[Register::a2];
            let arg3 = trap_frame[Register::a3];
            (trap_frame[Register::a0], trap_frame[Register::a1]) =
                handle_syscall(nr, arg1, arg2, arg3);
            cpu::write_sepc(sepc + 4); // Skip the ecall instruction
        }
        _ => {
            if cause.is_stack_overflow(stval) {
                let guard_range = LinkerInformation::__start_stack_overflow_guard()
                    ..LinkerInformation::__start_kernel_stack();
                warn!(
                    "DANGER! STACK OVERFLOW DETECTED! stval={:p} inside guard page {:p}-{:p}",
                    stval as *const (),
                    guard_range.start as *const (),
                    guard_range.end as *const ()
                );
            }
            let current_process = get_current_process();
            if let Some(current_process) = current_process {
                let current_process = current_process.lock();
                panic!(
                    "Unhandled exception!\nName: {}\nException code: {}\nstval: 0x{:x}\nsepc: 0x{:x}\nFrom Userspace: {}\nProcess name: {}\nTrap Frame: {:?}",
                    cause.get_reason(),
                    cause.get_exception_code(),
                    stval,
                    sepc,
                    current_process.get_page_table().is_userspace_address(sepc),
                    current_process.get_name(),
                    trap_frame
                );
            } else {
                panic!(
                    "Unhandled exception!\nName: {}\nException code: {}\nstval: 0x{:x}\nsepc: 0x{:x}\nFrom Userspace: {}\nProcess name: {}\nTrap Frame: {:?}",
                    cause.get_reason(),
                    cause.get_exception_code(),
                    stval,
                    sepc,
                    false,
                    "No scheduled process",
                    trap_frame
                );
            }
        }
    }
}

fn handle_interrupt(cause: InterruptCause, _stval: usize, _sepc: usize, _trap_frame: &TrapFrame) {
    match cause.get_exception_code() {
        SUPERVISOR_TIMER_INTERRUPT => handle_supervisor_timer_interrupt(),
        SUPERVISOR_EXTERNAL_INTERRUPT => handle_external_interrupt(),
        _ => {
            panic!("Unknwon interrupt! (Name: {})", cause.get_reason());
        }
    }
}

fn handle_supervisor_timer_interrupt() {
    scheduler::schedule();
}

fn handle_external_interrupt() {
    debug!("External interrupt occurred!");
    let plic_interrupt = plic::get_next_pending().expect("There should be a pending interrupt.");
    assert!(
        plic_interrupt == InterruptSource::Uart,
        "Plic interrupt should be uart."
    );

    let input = uart::read().expect("There should be input from the uart.");

    match input {
        4 => crate::debugging::dump_current_state(),
        _ => STDIN_BUFFER.lock().push(input),
    }

    plic::complete_interrupt(plic_interrupt);
}
