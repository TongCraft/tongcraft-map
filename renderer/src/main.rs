mod cli;

fn main() {
    if let Err(error) = cli::run() {
        eprintln!("错误：{error}");
        std::process::exit(1);
    }
}
