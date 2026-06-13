fn main() {
    if let Err(error) = cli::main_entry() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
