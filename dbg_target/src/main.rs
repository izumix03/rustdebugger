use std::arch::asm;

use nix::{
    sys::signal::{kill, Signal},
    unistd::getpid,
};

fn main() {
    println!("int 3");
    // unsafe { asm!("int 3") };
    unsafe { asm!(".inst 0xd4200000") }; // Apple Silicon 用

    println!("kill -SIGTRAP");
    let pid = getpid();
    kill(pid, Signal::SIGTRAP).unwrap();

    for i in 0..3 {
        unsafe { asm!("nop") };
        println!("i = {i}");
    }
}