//! Where a download the network service saved ends up: the agent
//! session's own directory below the host's downloads directory.

use super::page::Env;

/// Moves `file` the network service saved into the session's own
/// directory; what the reply says of it.
pub(crate) fn place(file: &str, bytes: u64, env: &Env) -> String {
    let Some(dir) = env.net.and_then(|n| n.downloads.clone()) else {
        return format!("downloaded {file} ({bytes} bytes)");
    };
    let mine = dir.join(format!("s{}-{}", env.owner, env.run));
    let to = mine.join(file);
    match std::fs::create_dir_all(&mine).and_then(|()| std::fs::rename(dir.join(file), &to)) {
        Ok(()) => format!("downloaded {file} ({bytes} bytes) to {}", to.display()),
        Err(e) => format!("downloaded {file} ({bytes} bytes); moving it failed: {e}"),
    }
}
