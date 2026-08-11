use multplx_cli::Cli;

fn main() {
    std::process::exit(Cli::parse_multicall(std::env::args_os()).run());
}
