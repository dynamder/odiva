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
    let mut lines: Vec<String> = Vec::new();

    if let Some(line) = unwritable_home() {
        lines.push(line);

        if let Some(marks) = spawned_by_dsh() {
            lines.push(format!(
                "这个 odiva 是从 dsh 里起的（{marks}），带着 dsh 只读的沙箱令牌，它派出的 dsh 也一样：\
                 换成 dsh 之外的终端（alacritty / 资源管理器）启动"
            ));
        }
    }

    lines.extend(proxy());
    lines.extend(desktop_shim());

    lines
}

/// Whether the harness would run inside the desktop app's own sandbox.
///
/// The desktop install puts its launcher on `PATH` ahead of any standalone CLI,
/// and that launcher boots the profile through the app — with the app's sandbox,
/// where `~/.dsh` is read-only. So `dsh` on this machine is not a harness: it
/// fails at the handshake, saying EPERM on `profiles/<name>/cordis.yml`.
fn desktop_shim() -> Option<String> {
    let program = env::var("ODIVA_HARNESS").unwrap_or_else(|_| "dsh".to_owned());

    if !program.to_lowercase().contains("dsh") {
        return None;
    }

    let found = candidates(Path::new(&program));
    let shim = found.iter().find(|path| is_desktop(path))?;

    Some(match found.iter().find(|path| !is_desktop(path)) {
        Some(standalone) => format!(
            "harness 解析到桌面版的 {}：它在桌面应用自己的沙箱里跑 profile，~/.dsh 只读。\
             改用独立 CLI：ODIVA_HARNESS={}",
            shim.display(),
            standalone.display()
        ),
        None => format!(
            "harness 解析到桌面版的 {}：它在桌面应用自己的沙箱里跑 profile，~/.dsh 只读。\
             装一个独立的 dsh（npm i -g @deepseek-ai/dsh），或把 ODIVA_DSH_HOME 指到一个可写目录",
            shim.display()
        ),
    })
}

/// A launcher that boots through the desktop host, which is where the sandbox
/// boundary comes from.
fn is_desktop(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| text.contains("dsh-desktop-host"))
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

    let (home, source) = dsh_home()?;

    let target = if home.is_dir() {
        home.clone()
    } else {
        home.parent()?.to_path_buf()
    };

    let error = probe(&target).err()?;

    let mut line = format!(
        "DSH_HOME {} 不可写（{error}，来自 {source}）：dsh 每次启动都要写 profiles/<name>/cordis.yml，\
         会在握手前退出。把 ODIVA_DSH_HOME 指到一个可写目录",
        home.display()
    );

    line.push_str(&match writable_elsewhere() {
        Some(where_) => format!(
            "；{where_} 写得进去，所以被拦的是这个路径，不是进程：多半是第三方防护（火绒 / Defender）\
             或组策略按路径拦截，把 odiva 和它派出的 dsh 加进信任"
        ),
        None => "；工作目录也写不了，是这个进程整体受限".to_owned(),
    });

    Some(line)
}

/// A place this process can write that is not the harness home, if there is one.
fn writable_elsewhere() -> Option<String> {
    let here = std::env::current_dir().ok()?;

    probe(&here).ok()?;

    Some(here.display().to_string())
}

/// Where dsh keeps its home, and which variable said so.
fn dsh_home() -> Option<(PathBuf, &'static str)> {
    if let Some(path) = env::var_os("ODIVA_DSH_HOME") {
        return Some((PathBuf::from(path), "ODIVA_DSH_HOME"));
    }

    if let Some(path) = env::var_os("DSH_HOME") {
        return Some((PathBuf::from(path), "DSH_HOME"));
    }

    std::env::var_os("USERPROFILE").map(|profile| (PathBuf::from(profile).join(".dsh"), "默认"))
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
