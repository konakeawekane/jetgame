//! Rust-to-Python pipe integration. All sockets and protocol handling are in Python.

use jetgame::networking::{
    HostOptions, LinkScope, NetworkClient, NetworkConfig, NetworkEvent, Request,
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn config() -> NetworkConfig {
    let mut config = NetworkConfig {
        root: Path::new(env!("CARGO_MANIFEST_DIR")).into(),
        ..Default::default()
    };
    config.set("host_port", 0);
    config.set("listen_ipv4", "127.0.0.1");
    config.set("listen_ipv6", "::1");
    config.set("lan_enabled", false);
    config
}
#[track_caller]
fn wait(client: &mut NetworkClient, accept: impl Fn(&NetworkEvent) -> bool) -> NetworkEvent {
    let deadline = Instant::now() + Duration::from_secs(8);
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
    panic!("Network event deadline exceeded");
}

#[test]
#[ignore = "requires Python 3.11+ and local sockets"]
fn python_agent_hosts_and_joins_without_a_directory() {
    let mut host = NetworkClient::new(config());
    host.submit(Request::Host {
        op: 1,
        options: HostOptions {
            name: "PIPE HOST".into(),
            max_players: 8,
            map: "WORLD 01".into(),
            mode: "TEAM DOGFIGHT".into(),
        },
    })
    .unwrap();
    let (mut endpoint, mut invitation) = (None, None);
    let deadline = Instant::now() + Duration::from_secs(8);
    while endpoint.is_none() || invitation.is_none() {
        for event in host.poll() {
            match event {
                NetworkEvent::Connected {
                    info,
                    endpoint: address,
                    ..
                } => {
                    assert!(info.host);
                    assert_eq!(info.name, "PIPE HOST");
                    endpoint = Some(address);
                }
                NetworkEvent::ShareLink { link, scope, .. } => {
                    assert!(link.starts_with("jetgame://join?"));
                    assert_eq!(scope, LinkScope::Local);
                    invitation = Some(link);
                }
                NetworkEvent::Error { message, .. } => panic!("{message}"),
                _ => {}
            }
        }
        assert!(
            Instant::now() < deadline,
            "host must connect and generate an invitation"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let endpoint = endpoint.unwrap();
    let mut guest = NetworkClient::new(config());
    guest
        .submit(Request::Join {
            op: 2,
            endpoint: invitation.unwrap(),
        })
        .unwrap();
    let NetworkEvent::Connected { info, .. } =
        wait(&mut guest, |e| matches!(e, NetworkEvent::Connected { .. }))
    else {
        unreachable!()
    };
    assert!(!info.host);
    guest.submit(Request::Ready { op: 2, ready: true }).unwrap();
    wait(
        &mut guest,
        |e| matches!(e,NetworkEvent::Lobby { players,.. } if players.len()==2 && players.iter().all(|p|p.ready)),
    );
    host.submit(Request::Start { op: 1 }).unwrap();
    wait(
        &mut host,
        |e| matches!(e,NetworkEvent::Error { message,.. } if message.contains("not implemented")),
    );
    host.submit(Request::Leave).unwrap();
    wait(&mut guest, |e| {
        matches!(e, NetworkEvent::Disconnected { .. })
    });
    assert!(std::net::TcpStream::connect(endpoint).is_err());
}

#[test]
#[ignore = "requires Python 3.11+"]
fn python_configuration_failures_are_reported_to_rust() {
    let mut config = config();
    config.set("host_port", -1);
    let mut client = NetworkClient::new(config);
    wait(
        &mut client,
        |e| matches!(e,NetworkEvent::Error { message,.. } if message.contains("host_port")),
    );
}
