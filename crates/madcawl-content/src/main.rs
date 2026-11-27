//! `madcawl-content --serve <socket> [serve options]`: the windowless
//! host of `madcawl-shell --serve`, with the scripts of the documents it
//! loads over the network run on this executable's engine. It always
//! runs as with `--isolate`: each session's pages live in a content
//! process (`--content`) and the network service in one of its own
//! (`--netd`), both this executable, so a script that never returns or
//! an engine that crashes ends one content process, which the host
//! ends and replaces, and never the host or another session.

use std::process::ExitCode;

use madcawl_shell::serve;

fn main() -> ExitCode {
    serve::scripts::set_scripts(madcawl_content::open);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["--serve", rest @ ..] => match serve::parse_args(rest) {
            Some(c) => match isolated(c).and_then(serve::Server::bind).and_then(serve::Server::run) {
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

/// `c` with its pages in content processes of this executable.
fn isolated(mut c: serve::Config) -> std::io::Result<serve::Config> {
    if c.isolate.is_none() {
        c.isolate = Some(std::env::current_exe()?);
    }
    if c.hang_first() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, serve::HANG_FIRST));
    }
    Ok(c)
}

fn usage() -> ExitCode {
    eprintln!("usage: madcawl-content --serve <socket> [options] (always with --isolate)\n{}", serve::USAGE);
    ExitCode::from(2)
}
