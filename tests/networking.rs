//! Explicit Rust/Python loopback integration: cargo test --test networking -- --ignored --nocapture

use jetgame::networking::{
    HostOptions, NetworkClient, NetworkConfig, NetworkEvent, Request, discover,
};
use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

struct DirectoryProcess(Child);
impl Drop for DirectoryProcess {
    fn drop(&mut self) {
        drop(self.0.stdin.take());
        for _ in 0..40 {
            if self.0.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn directory() -> (DirectoryProcess, String) {
    let mut command = Command::new(if cfg!(windows) { "python" } else { "python3" });
    command
        .arg("-I")
        .arg("-u")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("networking/server_main.py"))
        .args(["--service", "directory", "--parent-stdin", "--port", "0"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = DirectoryProcess(command.spawn().expect("Python interpreter"));
    let stdout = child.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line).unwrap();
        let _ = tx.send(line);
    });
    let line = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("directory startup");
    let ready: serde_json::Value = serde_json::from_str(&line).unwrap();
    (
        child,
        format!("127.0.0.1:{}", ready["port"].as_u64().unwrap()),
    )
}

fn wait(client: &mut NetworkClient, accept: impl Fn(&NetworkEvent) -> bool) -> NetworkEvent {
    let deadline = Instant::now() + Duration::from_secs(6);
    while Instant::now() < deadline {
        for event in client.poll() {
            if accept(&event) {
                return event;
            }
            if let NetworkEvent::Error { message, .. } = event {
                panic!("{message}");
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("network event deadline exceeded");
}

#[test]
#[ignore = "requires Python 3.11+ and local sockets"]
fn native_client_hosts_discovers_joins_and_cleans_up_python_server() {
    let (_directory, endpoint) = directory();
    let config = NetworkConfig {
        root: Path::new(env!("CARGO_MANIFEST_DIR")).into(),
        directory: Some(endpoint.clone()),
        host_bind: "127.0.0.1".into(),
        host_port: 0,
        ..Default::default()
    };
    let mut host = NetworkClient::new(config.clone());
    host.submit(Request::Host {
        op: 1,
        options: HostOptions {
            name: "RUST HOST".into(),
            max_players: 8,
            map: "WORLD 01".into(),
            mode: "TEAM DOGFIGHT".into(),
        },
    })
    .unwrap();
    let NetworkEvent::Connected {
        info,
        endpoint: match_endpoint,
        ..
    } = wait(&mut host, |e| matches!(e, NetworkEvent::Connected { .. }))
    else {
        unreachable!()
    };
    assert!(info.host);
    assert_eq!(info.name, "RUST HOST");
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if discover(&endpoint)
            .unwrap()
            .iter()
            .any(|s| s.endpoint == match_endpoint)
        {
            break;
        }
        assert!(Instant::now() < deadline, "host should advertise itself");
        std::thread::sleep(Duration::from_millis(20));
    }
    // A deliberately missing interpreter proves that ordinary clients never need Python.
    let mut guest = NetworkClient::new(NetworkConfig {
        python: Some("does-not-exist.exe".into()),
        ..config
    });
    guest.submit(Request::Discover { op: 2 }).unwrap();
    let NetworkEvent::Servers { servers, .. } =
        wait(&mut guest, |e| matches!(e, NetworkEvent::Servers { .. }))
    else {
        unreachable!()
    };
    assert_eq!(servers[0].name, "RUST HOST");
    guest
        .submit(Request::Join {
            op: 3,
            endpoint: match_endpoint.clone(),
        })
        .unwrap();
    let NetworkEvent::Connected { info, .. } =
        wait(&mut guest, |e| matches!(e, NetworkEvent::Connected { .. }))
    else {
        unreachable!()
    };
    assert!(!info.host);
    guest.submit(Request::Ready { op: 3, ready: true }).unwrap();
    wait(
        &mut guest,
        |e| matches!(e,NetworkEvent::Lobby{players,..}if players.len()==2&&players.iter().all(|p|p.ready)),
    );
    host.submit(Request::Start { op: 1 }).unwrap();
    let event = wait(
        &mut host,
        |e| matches!(e,NetworkEvent::Error{message,..}if message.contains("not implemented")),
    );
    assert!(matches!(event, NetworkEvent::Error { .. }));
    host.submit(Request::Leave).unwrap();
    wait(&mut guest, |e| {
        matches!(e, NetworkEvent::Disconnected { .. })
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if discover(&endpoint).unwrap().is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "shutdown should remove advertisement"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(std::net::TcpStream::connect(&match_endpoint).is_err());
}

#[test]
#[ignore = "requires Python 3.11+ and local sockets"]
fn native_discovery_fetches_multiple_directory_pages() {
    use jetgame::networking::protocol::{Message, exchange};
    use serde_json::json;
    let (_directory, endpoint) = directory();
    for index in 0..101 {
        let mut stream = std::net::TcpStream::connect(&endpoint).unwrap();
        let reply = exchange(
            &mut stream,
            &Message::new(
                "register",
                json!({
                    "port": 29010, "name": format!("MATCH {index}"), "max_players": 8
                }),
            ),
        )
        .unwrap();
        assert_eq!(reply.kind, "registered");
    }
    let listings = discover(&endpoint).unwrap();
    assert_eq!(listings.len(), 101);
    assert_eq!(listings[100].name, "MATCH 100");
}
