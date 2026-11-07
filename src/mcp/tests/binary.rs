//! The built `mcp` binary, end to end (env cleared, isolated XDG dirs).
mod common;

use std::{
    os::{fd::AsRawFd, unix::{fs::PermissionsExt, net::UnixDatagram, process::CommandExt}},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use common::{serve_mock, text_of, tool};
use rmcp::{model::{CallToolRequestParams, CallToolResponse}, ServiceExt};
use tokio::net::UnixStream;

const MCP: &str = env!("CARGO_BIN_EXE_mcp");

struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new(config: &str) -> Home {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("cfg/rafa")).unwrap();
        std::fs::write(dir.path().join("cfg/rafa/mcp.yaml"), config).unwrap();
        std::fs::create_dir_all(dir.path().join("run/rafa/mcp/servers")).unwrap();
        Home { dir }
    }
    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }
    fn pool(&self) -> PathBuf {
        self.path("run/rafa/mcp/servers")
    }
    fn socket(&self) -> PathBuf {
        self.path("run/rafa/mcp/proxy.sock")
    }
    fn env(&self) -> Vec<(String, String)> {
        let coverage = std::env::var("LLVM_PROFILE_FILE").ok().map(|v| ("LLVM_PROFILE_FILE".to_string(), v));
        coverage.into_iter().chain([
            ("HOME".into(), self.dir.path().display().to_string()),
            ("XDG_CONFIG_HOME".into(), self.path("cfg").display().to_string()),
            ("XDG_RUNTIME_DIR".into(), self.path("run").display().to_string()),
            ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
        ])
        .collect()
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(MCP);
        c.args(args).env_clear().envs(self.env());
        c
    }
    fn tokio_command(&self, args: &[&str]) -> tokio::process::Command {
        let mut c = tokio::process::Command::new(MCP);
        c.args(args).env_clear().envs(self.env());
        c
    }
    async fn in_process_proxy(&self) -> mcp::proxy::Proxy {
        let env = self.env().into_iter().collect();
        let config = mcp::system::load(&env, None).unwrap();
        let proxy = mcp::proxy::Proxy::new(config);
        proxy.sync_pool(&self.pool()).await;
        tokio::spawn(proxy.clone().run(tokio::net::UnixListener::bind(self.socket()).unwrap()));
        proxy
    }
}

/// Kills the child if the test panics before reaping it.
struct Reaped(Option<std::process::Child>);

impl Reaped {
    fn id(&self) -> u32 {
        self.0.as_ref().unwrap().id()
    }
    async fn wait(mut self) -> std::process::ExitStatus {
        let mut child = self.0.take().unwrap();
        tokio::time::timeout(Duration::from_secs(15), tokio::task::spawn_blocking(move || child.wait().unwrap()))
            .await
            .expect("child exit")
            .unwrap()
    }
}

impl Drop for Reaped {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn recv(sock: &UnixDatagram) -> String {
    let mut buf = [0u8; 256];
    let n = sock.recv(&mut buf).expect("a notify message");
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

async fn wait_for(path: &Path) {
    for _ in 0..100 {
        if UnixStream::connect(path).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{} never accepted", path.display());
}

fn call(name: &str) -> CallToolRequestParams {
    CallToolRequestParams::new(name.to_string())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn proxy_notifies_serves_discovers_and_drains_on_sigterm() {
    let home = Home::new("servers:\n  slack: { prefix: chat }\n");
    let _slack = serve_mock(&home.pool().join("slack.sock"), vec![tool("search")]);
    let notify = UnixDatagram::bind(home.path("notify")).unwrap();
    notify.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let child = Reaped(Some(home.command(&["proxy"]).env("NOTIFY_SOCKET", home.path("notify")).spawn().unwrap()));
    assert!(recv(&notify).contains("READY=1"));
    let mode = std::fs::metadata(home.socket()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "proxy socket mode {mode:o}");

    let agent = ().serve(UnixStream::connect(home.socket()).await.unwrap()).await.unwrap();
    let names = |tools: Vec<rmcp::model::Tool>| tools.iter().map(|t| t.name.to_string()).collect::<Vec<_>>();
    assert_eq!(names(agent.list_all_tools().await.unwrap()), vec!["chat-search"]);

    let _web = serve_mock(&home.pool().join("web.sock"), vec![tool("browse")]);
    let mut found = false;
    for _ in 0..40 {
        if names(agent.list_all_tools().await.unwrap()).contains(&"web-browse".to_string()) {
            found = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(found, "inotify never brought web in");

    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    assert!(recv(&notify).contains("STOPPING=1"));
    let status = child.wait().await;
    assert!(status.success(), "{status}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn proxy_refuses_a_second_instance() {
    let home = Home::new("");
    let _first = Reaped(Some(home.command(&["proxy"]).spawn().unwrap()));
    wait_for(&home.socket()).await;
    let out = home.command(&["proxy"]).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("already"), "{}", String::from_utf8_lossy(&out.stderr));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_is_a_stdio_mcp_server_for_agents() {
    let home = Home::new("profiles:\n  comms: { servers: { slack: { include: [search] } } }\n");
    let _slack = serve_mock(&home.pool().join("slack.sock"), vec![tool("search"), tool("post")]);
    let _proxy = home.in_process_proxy().await;
    for args in [vec!["client", "--profile", "comms"], vec!["client", "--server", "slack", "--include", "search"]] {
        let child = rmcp::transport::TokioChildProcess::new(home.tokio_command(&args)).unwrap();
        let agent = ().serve(child).await.unwrap();
        let tools = agent.list_all_tools().await.unwrap();
        assert_eq!(tools.iter().map(|t| t.name.as_ref()).collect::<Vec<_>>(), vec!["slack-search"], "{args:?}");
        let CallToolResponse::Complete(r) = agent.call_tool_once(call("slack-search")).await.unwrap() else { panic!() };
        assert!(text_of(&r).contains("called search"));
        agent.cancel().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_reports_an_unknown_profile() {
    let home = Home::new("profiles:\n  comms: { servers: { slack: {} } }\n");
    let out = home.command(&["client", "--profile", "nope"]).stdin(Stdio::null()).output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("nope") && err.contains("comms"), "{err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sidecar_serves_a_stdio_server_on_the_activated_socket() {
    let home = Home::new("");
    let listener = std::os::unix::net::UnixListener::bind(home.path("echo.sock")).unwrap();
    let fd = listener.as_raw_fd();
    let notify = UnixDatagram::bind(home.path("notify")).unwrap();
    notify.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut c = Command::new("sh");
    c.args(["-c", "LISTEN_PID=$$ exec \"$0\" \"$@\"", MCP, "sidecar", "--name", "echo", "--", "cat"])
        .env_clear()
        .envs(home.env())
        .env("LISTEN_FDS", "1")
        .env("NOTIFY_SOCKET", home.path("notify"));
    // SAFETY: dup2 is async-signal-safe; it places the listener at SD_LISTEN_FDS_START.
    unsafe {
        c.pre_exec(move || if libc::dup2(fd, 3) < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) });
    }
    let child = Reaped(Some(c.spawn().unwrap()));
    assert!(recv(&notify).contains("READY=1"));
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let mut s = tokio::io::BufReader::new(UnixStream::connect(home.path("echo.sock")).await.unwrap());
    s.get_mut().write_all(b"ping\n").await.unwrap();
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(5), s.read_line(&mut line)).await.unwrap().unwrap();
    assert_eq!(line, "ping\n");
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let status = child.wait().await;
    assert!(status.success(), "{status}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sidecar_without_activation_is_an_error() {
    let home = Home::new("");
    let out = home.command(&["sidecar", "--name", "x", "--", "cat"]).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("LISTEN_FDS"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn doctor_reports_json_and_fails_on_findings() {
    let home = Home::new("profiles:\n  comms: { servers: { slack: { include: [search, gone] } } }\n");
    let dead = home.command(&["doctor"]).output().unwrap();
    assert!(!dead.status.success());
    assert!(String::from_utf8_lossy(&dead.stdout).contains("[-] proxy"), "{}", String::from_utf8_lossy(&dead.stdout));

    let _slack = serve_mock(&home.pool().join("slack.sock"), vec![tool("search"), tool("post")]);
    let _proxy = home.in_process_proxy().await;
    let out = tokio::task::spawn_blocking({
        let mut c = home.command(&["doctor", "--json"]);
        move || c.output().unwrap()
    })
    .await
    .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["ok"], false, "{json}");
    assert_eq!(json["profiles"][0]["stale"][0]["rule"], "gone");
    assert_eq!(json["profiles"][0]["exposed"][0], "slack-search");
    assert!(!out.status.success());
}

#[test]
fn usage_errors_exit_2_and_version_prints() {
    let home = Home::new("");
    assert_eq!(home.command(&["teleport"]).output().unwrap().status.code(), Some(2));
    let v = home.command(&["--version"]).output().unwrap();
    assert!(String::from_utf8_lossy(&v.stdout).starts_with("mcp 0."));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn proxy_replaces_a_stale_socket_and_retries_dead_pool_entries() {
    let home = Home::new("");
    std::fs::create_dir_all(home.socket().parent().unwrap()).unwrap();
    drop(std::os::unix::net::UnixListener::bind(home.socket()).unwrap()); // stale: file, no listener
    drop(std::os::unix::net::UnixListener::bind(home.pool().join("dead.sock")).unwrap());
    let child = Reaped(Some(home.command(&["proxy"]).env("NOTIFY_SOCKET", home.path("no-such-notify")).spawn().unwrap()));
    wait_for(&home.socket()).await;
    std::fs::remove_file(home.pool().join("dead.sock")).unwrap();
    let _alive = serve_mock(&home.pool().join("dead.sock"), vec![tool("up")]);
    let agent = ().serve(UnixStream::connect(home.socket()).await.unwrap()).await.unwrap();
    let mut seen = false;
    for _ in 0..80 {
        if agent.list_all_tools().await.unwrap().iter().any(|t| t.name == "dead-up") {
            seen = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(seen, "the revived server never appeared");
    unsafe { libc::kill(child.id() as i32, libc::SIGINT) };
    assert!(child.wait().await.success());
}

#[test]
fn proxy_refuses_a_non_socket_in_its_place() {
    let home = Home::new("");
    std::fs::create_dir_all(home.socket().parent().unwrap()).unwrap();
    std::fs::write(home.socket(), "precious").unwrap();
    let out = home.command(&["proxy"]).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a socket"));
    assert_eq!(std::fs::read_to_string(home.socket()).unwrap(), "precious");
}

#[test]
fn config_edits_the_user_file_atomically_and_refuses_nix_entries() {
    let home = Home::new("version: 1\n");
    std::fs::create_dir_all(home.path("cfg/rafa/mcp.d")).unwrap();
    std::fs::write(home.path("cfg/rafa/mcp.d/10-nix.yaml"), "servers:\n  slack: { prefix: chat }\n").unwrap();
    let run = |args: &[&str]| home.command(args).output().unwrap();

    let out = run(&["config", "add-mcp", "scratch", "--command", "my-mcp", "--include", "read_*", "--", "--stdio"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stderr).contains("[+]"));
    let out = run(&["config", "add-profile", "work", "--server", "scratch", "--server", "slack", "--include", "slack:search_*"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let user = home.path("cfg/rafa/mcp.yaml");
    assert_eq!(std::fs::metadata(&user).unwrap().permissions().mode() & 0o777, 0o600);
    let text = std::fs::read_to_string(&user).unwrap();
    assert!(text.contains("my-mcp") && text.contains("work") && !text.contains("chat"), "{text}");
    let leftovers: Vec<_> = std::fs::read_dir(home.path("cfg/rafa")).unwrap().filter_map(Result::ok).map(|e| e.file_name()).filter(|n| n.to_string_lossy().contains(".tmp")).collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");

    let out = run(&["config", "remove-mcp", "slack"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("10-nix.yaml"));
    let out = run(&["config", "add-mcp", "slack"]);
    assert!(!out.status.success());

    let show = String::from_utf8_lossy(&run(&["config", "show", "--origin"]).stdout).into_owned();
    assert!(show.contains("servers.slack.prefix\t") && show.contains("10-nix.yaml") && show.contains("mcp.yaml"), "{show}");
    assert!(run(&["config", "remove-profile", "work"]).status.success());
    assert!(run(&["config", "remove-mcp", "scratch"]).status.success());
    let shown = String::from_utf8_lossy(&run(&["config", "show"]).stdout).into_owned();
    assert!(!shown.contains("scratch") && shown.contains("chat"), "{shown}");
}

#[test]
fn config_tui_needs_a_terminal() {
    let home = Home::new("");
    let out = home.command(&["config-tui"]).stdin(Stdio::null()).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("terminal"), "{}", String::from_utf8_lossy(&out.stderr));
}

/// `script` gives the TUI a real pseudo-terminal; keys are written to it with pauses.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn config_tui_toggles_and_saves_through_a_real_terminal() {
    let home = Home::new("");
    let remote = home.path("mine.sock");
    let _m = serve_mock(&remote, vec![tool("alpha"), tool("beta")]);
    std::fs::write(home.path("cfg/rafa/mcp.yaml"), format!("servers:\n  mine: {{ transport: {{ unix: \"{}\" }} }}\n", remote.display())).unwrap();
    let command = format!("stty cols 110 rows 20; exec {MCP} config-tui");
    let mut child = tokio::process::Command::new("script")
        .args(["-qfec", &command, "/dev/null"])
        .env_clear()
        .envs(home.env())
        .env("TERM", "xterm-256color")
        // script runs the command with $SHELL, else the passwd shell (/noshell in the Nix sandbox)
        .env("SHELL", "/bin/sh")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("script (util-linux) on PATH");
    use tokio::io::AsyncWriteExt;
    let mut keys = child.stdin.take().unwrap();
    // open mine, hide alpha, save (s), quit (q)
    for key in ["", "\r", " ", "s", "q"] {
        tokio::time::sleep(Duration::from_millis(700)).await;
        if keys.write_all(key.as_bytes()).await.and(keys.flush().await).is_err() {
            break; // script is gone; its output below says why
        }
    }
    let out = tokio::time::timeout(Duration::from_secs(15), child.wait_with_output()).await.expect("the TUI quit").unwrap();
    let screen = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{} stdout: {screen:?} stderr: {:?}", out.status, String::from_utf8_lossy(&out.stderr));
    assert!(screen.contains("mine-alpha"), "the tools screen was drawn: {screen}");
    let saved = std::fs::read_to_string(home.path("cfg/rafa/mcp.yaml")).unwrap();
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&saved).unwrap();
    assert_eq!(value["servers"]["mine"]["exclude"], serde_yaml_ng::from_str::<serde_yaml_ng::Value>("[alpha]").unwrap(), "{saved}");
    assert_eq!(std::fs::metadata(home.path("cfg/rafa/mcp.yaml")).unwrap().permissions().mode() & 0o777, 0o600);
}
