#![cfg_attr(miri, allow(unused_imports))]
use crate::{println, test::qemu_exit};
use core::{panic::PanicInfo, sync::atomic::AtomicU8};

static PANIC_COUNTER: AtomicU8 = AtomicU8::new(0);

#[cfg(not(miri))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    abort_if_triple_fault();

    crate::cpu::disable_gloabl_interrupts();
    println!("");
    crate::debug::dump_current_state();
    println!("KERNEL Panic Occured!");
    println!("Message: {}", info.message());
    if let Some(location) = info.location() {
        println!("Location: {}", location);
    }

    qemu_exit::exit_failure(1);
}

fn abort_if_triple_fault() {
    let current = PANIC_COUNTER.fetch_add(1, core::sync::atomic::Ordering::SeqCst);

    if current >= 3 {
        println!("TRIPLE FAULT");
        qemu_exit::exit_failure(1);
    }
}
