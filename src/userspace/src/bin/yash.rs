#![no_std]
#![no_main]

use alloc::string::{String, ToString};
use common::syscalls::{sys_execute, sys_exit, sys_read_input, sys_wait};
use userspace::{print, println, util::wait};

extern crate alloc;
extern crate userspace;

const DELETE: u8 = 127;

#[no_mangle]
fn main() {
    println!();
    println!("### YaSH - Yet another Shell ###");
    println!("Type 'help' for a list of available commands.");
    loop {
        print!("$ ");
        let mut input = String::new();
        loop {
            let result = loop {
                if let Some(c) = sys_read_input() {
                    break c;
                }
                wait(10000);
            };
            match result {
                b'\r' => {
                    // Carriage return
                    println!();
                    break;
                }
                DELETE => {
                    if input.pop().is_some() {
                        print!("{}{}{}", 8 as char, ' ', 8 as char);
                    }
                }
                _ => {
                    assert!(result.is_ascii());
                    let result = result as char;
                    input.push(result);
                    print!("{}", result);
                }
            }
        }
        // Parse input and execute
        parse_command_and_execute(input);
    }
}

fn parse_command_and_execute(mut command: String) {
    command = command.trim().to_string();
    match command.as_str() {
        "" => {}
        "exit" => {
            println!("Exiting...");
            sys_exit(0);
        }
        "help" => {
            println!("Available commands:");
            println!("exit - Exit the shell");
            println!("help - Print this help message");
        }
        _ => {
            let mut background = false;

            if command.ends_with('&') {
                background = true;
                command.pop();
                command = command.trim().to_string();
            }

            let reference = unsafe { &*command.as_ptr() };
            let len = command.len();

            let execute_result = sys_execute(reference, len);
            match execute_result {
                Ok(pid) => {
                    if !background {
                        let _ = sys_wait(pid);
                    }
                }
                Err(err) => {
                    println!("Error executing program: {:?}", err);
                }
            }
        }
    }
}
