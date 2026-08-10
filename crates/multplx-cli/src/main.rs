use multplx_cli::Cli;

fn main() {
    Cli::parse_multicall(std::env::args_os()).run();
}
