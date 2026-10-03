fn main() {
    let exit_code = worlddb_cli::cli::run(std::env::args_os().skip(1));
    std::process::exit(i32::from(exit_code));
}
