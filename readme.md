# YaROS (Yet another RISC-V Operating System)

[![Build](https://github.com/hieronymusma/yaros/actions/workflows/build.yml/badge.svg)](https://github.com/hieronymusma/yaros/actions/workflows/build.yml)
[![Test](https://github.com/hieronymusma/yaros/actions/workflows/test.yml/badge.svg)](https://github.com/hieronymusma/yaros/actions/workflows/test.yml)
[![Miri](https://github.com/hieronymusma/yaros/actions/workflows/miri.yml/badge.svg)](https://github.com/hieronymusma/yaros/actions/workflows/miri.yml)  
This projects makes my dream come true - write my own operating system. I'm doing this mostly for fun, so don't expect a fully-fledged operating system on basis of the RISC-V architecture.
Exactly like [SerenityOS](https://github.com/SerenityOS/serenity) this project doesn't use third-party runtime dependencies. If third-party dependencies are used, then only for the Build.

## Status

Implemented

- Page allocator
- Heap allocator
- Interrupt handling (PLIC -> UART interrupts)
- Testing harness
- Executing in supervisor mode
- Userspace processes
- Scheduler
- Systemcalls

TODO

- VirtIO / Filesystem
- Networkstack
- GUI
- See [todo](./todo.md)

## How do I run it?

To run the operating system you need to have the following tools installed:

- Rust
- qemu-system-riscv64

To install them on Ubuntu you can execute the following commands

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
sudo apt install qemu-system-riscv64
```

To run the operating system execute

```
cargo run --release
```

## What can I do?

Type `help` into the shell to get some information. If you type the name of a program it get's executed. If you add an ampersand at the end of the command it get's executed in the background. See `src/userspace/src/bin` for programs which can be executed.

## Justfile

The justfile contains useful commands which I often use. To run them you first need to install just (just a command runner).
`cargo install just`. To get a list of all commands execute `just -l`.
