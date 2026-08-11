use multplx_cli::Cli;
use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let status = Cli::parse_multicall(std::env::args_os()).run();
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    ExitCode::from(status as u8)
}
