fn main() {
    if let Err(error) = cellarium::app::run() {
        eprintln!("cellarium: {error}");
        std::process::exit(1);
    }
}
