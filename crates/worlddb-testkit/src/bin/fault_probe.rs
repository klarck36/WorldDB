#![forbid(unsafe_code)]

use std::env;
use std::io::{self, Write};
use std::process;
use worlddb_testkit::Seed;
use worlddb_testkit::fault::FaultHook;

fn main() {
    process::exit(run());
}

fn run() -> i32 {
    let mut arguments = env::args().skip(1);
    let seed_value = match (arguments.next().as_deref(), arguments.next()) {
        (Some("--seed"), Some(value)) if arguments.next().is_none() => value,
        _ => {
            let _ = writeln!(io::stderr(), "usage: fault-probe --seed <decimal-or-hex>");
            return 2;
        }
    };
    let seed = match Seed::parse(&seed_value) {
        Ok(seed) => seed,
        Err(error) => {
            let _ = writeln!(io::stderr(), "{error}");
            return 2;
        }
    };
    let mut hook = FaultHook::armed();
    match hook.trigger(seed, "m0-13-replay-probe") {
        Err(error) => {
            let _ = writeln!(io::stderr(), "FAULT_PROBE: {error}");
            73
        }
        Ok(()) => 0,
    }
}
