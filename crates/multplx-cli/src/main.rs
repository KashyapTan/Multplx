use multplx_cli::Cli;
use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::from(Cli::parse_multicall(std::env::args_os()).run() as u8)
}
