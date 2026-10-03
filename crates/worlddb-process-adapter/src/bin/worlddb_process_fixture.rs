#![deny(unsafe_code)]

use std::env;
use std::fs;
use std::process::{self, Command};
use std::thread;
use std::time::Duration;

fn main() {
    let mut args = env::args_os().skip(1);
    match args
        .next()
        .and_then(|value| value.into_string().ok())
        .as_deref()
    {
        Some("memory") => memory_probe(),
        Some("tree") => spawn_tree_child(args.next()),
        Some("late-write") => late_write(args.next()),
        Some("identity") => println!("{}", process::id()),
        _ => process::exit(2),
    }
}

fn memory_probe() -> ! {
    let mut small = Vec::new();
    if small.try_reserve_exact(8 * 1_024 * 1_024).is_err() {
        process::exit(41);
    }
    small.resize(8 * 1_024 * 1_024, 1_u8);

    let mut large = Vec::new();
    if large.try_reserve_exact(96 * 1_024 * 1_024).is_err() {
        process::exit(42);
    }
    large.resize(96 * 1_024 * 1_024, 1_u8);
    process::exit(if large.len() == 96 * 1_024 * 1_024 {
        0
    } else {
        43
    });
}

fn spawn_tree_child(marker: Option<std::ffi::OsString>) {
    let Some(marker) = marker else {
        process::exit(3);
    };
    let executable = match env::current_exe() {
        Ok(path) => path,
        Err(_) => process::exit(4),
    };
    if Command::new(executable)
        .arg("late-write")
        .arg(marker)
        .spawn()
        .is_err()
    {
        process::exit(5);
    }
    println!("ready");
}

fn late_write(marker: Option<std::ffi::OsString>) {
    let Some(marker) = marker else {
        process::exit(6);
    };
    let path = std::path::PathBuf::from(marker);
    if fs::write(path.join("started"), b"started").is_err() {
        process::exit(7);
    }
    thread::sleep(Duration::from_secs(2));
    if fs::write(path.join("late"), b"late").is_err() {
        process::exit(8);
    }
}
