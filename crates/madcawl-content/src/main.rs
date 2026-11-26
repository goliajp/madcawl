//! `madcawl-content --serve <socket> [serve options]`: the windowless
//! host of `madcawl-shell --serve`, with the scripts of the documents it
//! loads over the network run on this executable's engine. With
//! `--isolate` its content and network processes are this executable
//! too (`--content`, `--netd`).

use std::process::ExitCode;

use madcawl_shell::serve;

fn main() -> ExitCode {
    serve::scripts::set_scripts(madcawl_content::open);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["--serve", rest @ ..] => match serve::parse_args(rest) {
            Some(c) => match serve::Server::bind(c).and_then(serve::Server::run) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("madcawl-content: {e}");
                    ExitCode::FAILURE
                }
            },
            None => usage(),
        },
        ["--netd", rest @ ..] => serve::netproc::child_main(rest),
        ["--content", rest @ ..] => serve::content::child_main(rest),
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: madcawl-content --serve <socket> [options]\n{}", serve::USAGE);
    ExitCode::from(2)
}
