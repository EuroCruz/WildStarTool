fn main() -> std::process::ExitCode {
    wildstartool::cli::run(std::env::args().skip(1).collect())
}
