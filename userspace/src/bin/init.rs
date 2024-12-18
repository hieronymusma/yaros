#![no_std]
#![no_main]

use common::syscalls::{sys_execute, sys_wait};
use userspace::println;

extern crate userspace;

#[unsafe(no_mangle)]
fn main() {
    println!("init process started");
    println!("starting shell");
    let shell_name = "yash";
    let shell_pid = sys_execute(&shell_name.as_bytes()[0], shell_name.len()).unwrap();
    sys_wait(shell_pid as u64).unwrap();
    println!("Initial shell has exited...");
}
