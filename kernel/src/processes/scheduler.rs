use super::{
    process::{Pid, Process, ProcessState},
    process_list::add_process,
};
use crate::{
    autogenerated::userspace_programs::{INIT, PROGRAMS},
    cpu::{self, sret_to_kernel},
    debug, info,
    interrupts::{read_trap_frame, set_sscratch_to_kernel_trap_frame, write_trap_frame},
    klibc::elf::ElfFile,
    memory::page_tables::{activate_page_table, KERNEL_PAGE_TABLES},
    processes::{
        process_list::{self, notify_died},
        timer,
    },
    test::qemu_exit,
};
use alloc::sync::Arc;
use common::mutex::Mutex;

pub fn initialize() {
    let elf = ElfFile::parse(INIT).expect("Cannot parse ELF file");
    let process = Process::from_elf(&elf, "init");
    add_process(process);
    info!("Scheduler initialized and INIT process added to queue");
}

static CURRENT_PROCESS: Mutex<Option<Arc<Mutex<Process>>>> = Mutex::new(None);

pub unsafe fn disarm_current_process() {
    CURRENT_PROCESS.disarm();
}

pub fn get_current_process_expect() -> Arc<Mutex<Process>> {
    get_current_process()
        .expect("There must be a running current process")
        .clone()
}

pub fn get_current_process() -> Option<Arc<Mutex<Process>>> {
    CURRENT_PROCESS.lock().as_ref().map(|p| p.clone())
}

pub fn schedule() {
    debug!("Schedule next process");
    if prepare_next_process() {
        timer::set_timer(10);
        return;
    }
    activate_page_table(&KERNEL_PAGE_TABLES);
    timer::disable_timer();
    let addr = cpu::wfi_loop as *const () as usize;
    debug!("setting sepc={addr:#x}");
    cpu::write_sepc(addr);
    set_sscratch_to_kernel_trap_frame();
    sret_to_kernel()
}

pub fn kill_current_process() {
    debug!("Killing current process");
    let current_process = CURRENT_PROCESS.lock().take();
    if let Some(current_process) = current_process {
        activate_page_table(&KERNEL_PAGE_TABLES);
        notify_died(current_process.lock().get_pid());
        debug!("{:?}", current_process);
    }
    schedule();
}

pub fn let_current_process_wait_for(pid: Pid) -> bool {
    if !process_list::does_pid_exits(pid) {
        return false;
    }
    let current_process_lock = CURRENT_PROCESS.lock();
    let current_process = current_process_lock
        .as_ref()
        .expect("There should be a process.");

    let mut current_process = current_process.lock();
    current_process.set_state(ProcessState::WaitingFor(pid));
    current_process.set_syscall_return_code(0);
    true
}

pub fn let_current_process_wait_for_input() {
    let current_process_lock = CURRENT_PROCESS.lock();
    let current_process = current_process_lock
        .as_ref()
        .expect("There should be a process.");

    let mut current_process = current_process.lock();
    current_process.set_state(ProcessState::WaitingForInput);
}

pub fn send_ctrl_c() {
    queue_current_process_back();
    let highest_pid = process_list::get_highest_pid();

    if let Some(process) = highest_pid {
        let process_lock = process.lock();
        if process_lock.get_name() == "yash" {
            return;
        }
        let pid = process_lock.get_pid();
        activate_page_table(&KERNEL_PAGE_TABLES);
        drop(process_lock);
        drop(process);
        process_list::kill(pid);
    }

    schedule();
}

pub fn schedule_program(name: &str) -> Option<Pid> {
    for (prog_name, elf) in PROGRAMS {
        if name == *prog_name {
            let elf = ElfFile::parse(elf).expect("Cannot parse ELF file");
            let process = Process::from_elf(&elf, prog_name);
            let pid = process.get_pid();
            add_process(process);
            return Some(pid);
        }
    }
    None
}

fn queue_current_process_back() {
    let current_process = CURRENT_PROCESS.lock().take();

    if let Some(current_process) = current_process {
        {
            let mut current_process = current_process.lock();
            current_process.set_program_counter(cpu::read_sepc());
            current_process.set_in_kernel_mode(cpu::is_in_kernel_mode());
            current_process.set_register_state(&read_trap_frame());
            debug!(
                "Unscheduling PID={} NAME={}",
                current_process.get_pid(),
                current_process.get_name()
            );
        }
        process_list::enqueue(current_process);
    }
}

fn prepare_next_process() -> bool {
    queue_current_process_back();

    if process_list::is_empty() {
        info!("No more processes to schedule, shutting down system");
        qemu_exit::exit_success();
    }

    let next_process_ref = if let Some(process) = process_list::next_runnable() {
        process
    } else {
        return false;
    };

    {
        let next_process = next_process_ref.lock();

        let pc = next_process.get_program_counter();

        write_trap_frame(next_process.get_register_state());
        cpu::write_sepc(pc);
        cpu::set_ret_to_kernel_mode(next_process.get_in_kernel_mode());
        activate_page_table(next_process.get_page_table());

        debug!(
            "Scheduling PID={} NAME={}",
            next_process.get_pid(),
            next_process.get_name()
        );
    }

    *CURRENT_PROCESS.lock() = Some(next_process_ref);

    true
}
