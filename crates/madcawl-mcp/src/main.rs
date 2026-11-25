//! `madcawl-mcp [--socket <path>]`: the socket comes from `--socket`, else
//! `MADCAWL_SOCKET`.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let socket = match args.as_slice() {
        [flag, path] if flag == "--socket" => Some(PathBuf::from(path)),
        [] => std::env::var_os("MADCAWL_SOCKET").map(PathBuf::from),
        _ => None,
    };
    let Some(socket) = socket else {
        eprintln!("usage: madcawl-mcp [--socket <path>] (or MADCAWL_SOCKET in the environment)");
        return ExitCode::from(2);
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let mut server = madcawl_mcp::server::Server::new(socket, cwd);
    let stdin = std::io::stdin().lock();
    match server.serve(stdin, std::io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("madcawl-mcp: {e}");
            ExitCode::FAILURE
        }
    }
}
