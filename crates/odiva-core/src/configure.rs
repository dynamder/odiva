//! What the environment says about the harness to start, and what would stop
//! this machine from working before anything the user can see.
//!
//! `ODIVA_HARNESS` names the program, `ODIVA_HARNESS_ARGS` its arguments (space
//! separated). The default is dsh in its ACP profile — the only mode that speaks
//! the protocol on stdio. `ODIVA_DSH_PERMISSION_MODE` overrides the sandbox
//! boundary it starts with.
//!
//! dsh writes `profiles/<name>/cordis.yml` into its home on every boot, so a home
//! that refuses a new file is a dead harness. When the environment's home refuses
//! one, a home under the data root stands in rather than failing on the first
//! task; the log says so, and says where the credentials have to go.

use std::env;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use odiva_harness::{Harness, candidates};

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

    let home = home();
    let _ = std::fs::create_dir_all(&home.path);

    harness.env("DSH_HOME", home.path.display().to_string())
}

/// Where the core keeps its own files.
pub fn data_root() -> PathBuf {
    env::var_os("ODIVA_HOME").map(PathBuf::from).unwrap_or_else(|| {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".odiva")
    })
}

/// The facts about this environment that decide whether a turn can run at all.
///
/// They fail before anything reaches the window, so this is where they are said:
/// a log line naming the cause beats a symptom the user has to guess.
pub fn diagnostics() -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let home = home();

    if let Some((refused, from)) = &home.refused {
        lines.push(format!(
            "DSH_HOME {} 写不进去（来自 {from}）：dsh 每次启动都要写 profiles/<name>/cordis.yml。改用 {}",
            refused.display(),
            home.path.display()
        ));

        if let Some(marks) = spawned_by_dsh() {
            lines.push(format!(
                "这个 odiva 是从 dsh 里起的（{marks}），带着 dsh 的只读令牌，它派出的 dsh 也一样：\
                 换成 dsh 之外的终端（alacritty / 资源管理器）启动会更干净"
            ));
        }

        let credentials = refused.join(".credentials.yaml");

        if credentials.is_file() && !home.path.join(".credentials.yaml").is_file() {
            lines.push(format!(
                "{} 里没有凭据：worker 调模型时会说没有 key。把 {} 复制过去",
                home.path.display(),
                credentials.display()
            ));
        }
    }

    lines.extend(proxy());

    lines
}

/// The home dsh is given, and the one it was refused instead.
struct Home {
    path: PathBuf,
    /// The home the environment named, when it refused a new file.
    refused: Option<(PathBuf, &'static str)>,
}

fn home() -> Home {
    let Some((named, from)) = dsh_home() else {
        return Home {
            path: fallback(),
            refused: None,
        };
    };

    if probe(&writable_target(&named)).is_ok() {
        return Home {
            path: named,
            refused: None,
        };
    }

    Home {
        path: fallback(),
        refused: Some((named, from)),
    }
}

/// The program the harness is, as `PATH` resolves it.
///
/// A bare `dsh` may be the desktop app's bundled CLI rather than a standalone
/// one, so the startup log names the file that will actually run.
pub fn resolved() -> PathBuf {
    let program = env::var("ODIVA_HARNESS").unwrap_or_else(|_| "dsh".to_owned());

    candidates(Path::new(&program))
        .into_iter()
        .next()
        .unwrap_or_else(|| PathBuf::from(program))
}

/// dsh's own knob is honoured as given: moving it is the caller's decision, and
/// second-guessing it would hide a wrong path behind a home that happens to work.
fn dsh_home() -> Option<(PathBuf, &'static str)> {
    if let Some(path) = env::var_os("ODIVA_DSH_HOME") {
        return Some((PathBuf::from(path), "ODIVA_DSH_HOME"));
    }

    if let Some(path) = env::var_os("DSH_HOME") {
        return Some((PathBuf::from(path), "DSH_HOME"));
    }

    env::var_os("USERPROFILE").map(|profile| (PathBuf::from(profile).join(".dsh"), "默认"))
}

/// Where a home that cannot be written is replaced. Beside the core's own files,
/// because that is the one place this process is known to write.
fn fallback() -> PathBuf {
    data_root().join("harness-home")
}

/// The directory a probe writes into: the home itself, or the place it would be
/// created in.
fn writable_target(home: &Path) -> PathBuf {
    if home.is_dir() {
        return home.to_path_buf();
    }

    home.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.to_path_buf())
}

/// Whether a directory takes a new file. The probe is removed again.
fn probe(directory: &Path) -> std::io::Result<()> {
    let file = directory.join(".odiva-write-probe");

    std::fs::write(&file, b"")?;
    let _ = std::fs::remove_file(&file);

    Ok(())
}

/// Which dsh variables are in the environment, when any are.
///
/// dsh exports these to every shell it starts, and the confinement travels with
/// them: a window opened from such a shell, or from a terminal that shell
/// started, cannot write `~/.dsh` either.
fn spawned_by_dsh() -> Option<String> {
    let marks: Vec<String> = ["DSH_SHELL", "DSH_PROFILE", "DSH_SESSION_ID"]
        .into_iter()
        .filter_map(|name| env::var(name).ok().map(|value| format!("{name}={value}")))
        .collect();

    (!marks.is_empty()).then(|| marks.join("，"))
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
