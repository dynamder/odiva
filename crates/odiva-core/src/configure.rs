//! What the environment says about the harness to start, and what would stop
//! this machine from working before anything the user can see.
//!
//! `ODIVA_HARNESS` names the program, `ODIVA_HARNESS_ARGS` its arguments (space
//! separated). The default is dsh in its ACP profile — the only mode that speaks
//! the protocol on stdio. `ODIVA_DSH_HOME` moves dsh's own home when the default
//! one is not writable; `ODIVA_DSH_PERMISSION_MODE` overrides the sandbox
//! boundary it starts with.

use std::env;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use odiva_harness::Harness;

pub fn harness() -> Harness {
    let program = env::var("ODIVA_HARNESS").unwrap_or_else(|_| "dsh".to_owned());
    let args = env::var("ODIVA_HARNESS_ARGS").unwrap_or_else(|_| "--profile acp".to_owned());

    let mut harness = Harness::new(program);

    for arg in args.split_whitespace() {
        harness = harness.arg(arg);
    }

    harness = harness.env(
        "DSH_PERMISSION_MODE",
        env::var("ODIVA_DSH_PERMISSION_MODE").unwrap_or_else(|_| "workspace-write".to_owned()),
    );

    if let Ok(home) = env::var("ODIVA_DSH_HOME") {
        harness = harness.env("DSH_HOME", home);
    }

    harness
}

/// The facts about this environment that decide whether a turn can run at all.
///
/// Both of them fail before anything reaches the window, so this is where they
/// are said: a log line naming the cause beats a symptom the user has to guess.
pub fn diagnostics() -> Vec<String> {
    [unwritable_home(), proxy()].into_iter().flatten().collect()
}

/// A startup warning when dsh's own home would stop it from booting.
///
/// dsh writes `profiles/<name>/cordis.yml` on every boot, so a read-only home
/// fails before the handshake — with nothing in our log to say why.
fn unwritable_home() -> Option<String> {
    let program = env::var("ODIVA_HARNESS").unwrap_or_else(|_| "dsh".to_owned());

    if !program.to_lowercase().contains("dsh") {
        return None;
    }

    let home = env::var_os("ODIVA_DSH_HOME")
        .or_else(|| env::var_os("DSH_HOME"))
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".dsh")))?;

    let target = if home.is_dir() {
        home.clone()
    } else {
        home.parent()?.to_path_buf()
    };

    let probe = target.join(".odiva-write-probe");

    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            None
        }
        Err(error) => Some(format!(
            "DSH_HOME {} 不可写（{error}）：dsh 启动会在 cordis.yml 上失败。把 ODIVA_DSH_HOME 指到一个可写目录",
            home.display()
        )),
    }
}

/// The proxy every model request goes through, and whether anything answers it.
///
/// `reqwest` takes this from the environment on its own, so odiva never sees it
/// until a turn fails — and there the stale port is a `tcp connect error` at a
/// loopback address, which reads like a broken network instead of a stale
/// variable left behind when the proxy moved to another port.
fn proxy() -> Option<String> {
    const NAMES: [&str; 3] = ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY"];

    let (name, url) = NAMES
        .iter()
        .find_map(|name| value(name).map(|url| (*name, url)))?;

    let state = match endpoint(&url).and_then(answering) {
        Some(true) => "在听",
        Some(false) => "没有应答，模型请求会全部失败：改成代理当前的端口，或清掉这个变量再启动",
        None => "应答无法判断",
    };

    Some(format!("{name}={url}（{state}）"))
}

fn value(name: &str) -> Option<String> {
    env::var(name)
        .or_else(|_| env::var(name.to_ascii_lowercase()))
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// The address a proxy URL points at, defaulting to port 80 as `reqwest` does.
fn endpoint(url: &str) -> Option<(String, u16)> {
    let authority = url
        .trim()
        .rsplit_once("://")
        .map_or(url.trim(), |(_, rest)| rest)
        .split('/')
        .next()?;

    match authority.rsplit_once(':') {
        Some((host, port)) => Some((host.to_owned(), port.parse().ok()?)),
        None => Some((authority.to_owned(), 80)),
    }
}

/// Whether something listens there. Only a literal address is probed: resolving
/// a name at startup would block the window on DNS.
fn answering((host, port): (String, u16)) -> Option<bool> {
    let address: IpAddr = host.parse().ok()?;
    let socket = SocketAddr::new(address, port);

    Some(TcpStream::connect_timeout(&socket, Duration::from_millis(300)).is_ok())
}

#[cfg(test)]
mod tests {
    use super::endpoint;

    #[test]
    fn a_proxy_url_yields_its_address() {
        assert_eq!(
            endpoint("http://127.0.0.1:7900/"),
            Some(("127.0.0.1".to_owned(), 7900))
        );
        assert_eq!(
            endpoint("127.0.0.1:7897"),
            Some(("127.0.0.1".to_owned(), 7897))
        );
        assert_eq!(
            endpoint("socks5://proxy.local:1080"),
            Some(("proxy.local".to_owned(), 1080))
        );
        assert_eq!(
            endpoint("http://proxy.local"),
            Some(("proxy.local".to_owned(), 80))
        );
        assert_eq!(endpoint("http://127.0.0.1:http"), None);
    }
}
