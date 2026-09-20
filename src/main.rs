fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = nect::cli::execute(&args);
    std::process::exit(code);
}
