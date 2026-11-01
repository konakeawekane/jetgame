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

struct Replica {
    ui: Option<jetgame::ui::UiController>,
    client: NetworkClient,
    game: jetgame::game::multiplayer::Multiplayer,
    world: jetgame::game::World,
    config: jetgame::config::GameConfig,
    op: u64,
    connected: bool,
    ready: bool,
    link: Option<String>,
    endpoint: Option<String>,
    disconnected: bool,
}
impl Replica {
    fn new(op: u64) -> Self {
        let mut game_config = jetgame::config::GameConfig::default();
        game_config.rules.cover_count = 0;
        game_config.rules.round_seconds = 120.;
        let world = jetgame::game::World::configured(
            jetgame::terrain::Terrain::new(
                &jetgame::rendering::MeshData::default(),
                glam::Mat4::IDENTITY,
            ),
            &game_config,
        );
        Self {
            ui: None,
            client: NetworkClient::new(config()),
            game: Default::default(),
            world,
            config: game_config,
            op,
            connected: false,
            ready: false,
            link: None,
            endpoint: None,
            disconnected: false,
        }
    }
    fn send(&mut self, request: Request) {
        self.game.note_request(&request);
        self.client.submit(request).unwrap();
    }
    fn flush(&mut self) {
        if let Some(ui) = &mut self.ui {
            let requests = ui.take_network_requests();
            for request in requests {
                self.game.note_request(&request);
                self.client.submit(request).unwrap();
            }
        }
        for request in self.game.take_requests() {
            self.client.submit(request).unwrap();
        }
    }
    fn pump(&mut self) {
        for event in self.client.poll() {
            if self
                .ui
                .as_ref()
                .is_some_and(|ui| !ui.accepts_network_event(&event))
            {
                continue;
            }
            self.game
                .handle(&event, &mut self.world, &self.config)
                .unwrap();
            match &event {
                NetworkEvent::Connected { endpoint, .. } => {
                    self.connected = true;
                    self.endpoint = Some(endpoint.clone());
                }
                NetworkEvent::ShareLink { link, scope, .. } => {
                    assert_eq!(*scope, LinkScope::Local);
                    self.link = Some(link.clone());
                }
                NetworkEvent::Lobby { players, .. } => self.ready = players.iter().all(|p| p.ready),
                NetworkEvent::Error { message, .. } => panic!("{message}"),
                NetworkEvent::Disconnected { .. } => self.disconnected = true,
                _ => {}
            }
            if let Some(ui) = &mut self.ui {
                ui.network_event(event);
            }
        }
        self.flush();
    }
    fn step(&mut self, input: &jetgame::flight::FlightInput) {
        self.pump();
        self.game
            .advance(1. / 120., &mut self.world, input, 0, -glam::Vec3::Z, true);
        self.flush();
    }
    fn action(&mut self, id: u32, action: jetgame::game::net_types::GameAction, team: u8) {
        self.send(Request::Action {
            op: self.op,
            epoch: self.game.epoch().unwrap(),
            action_id: id,
            action,
            team,
        });
    }
}
fn drive(
    host: &mut Replica,
    guest: &mut Replica,
    frames: usize,
    input: &jetgame::flight::FlightInput,
) {
    for _ in 0..frames {
        host.step(&Default::default());
        guest.step(input);
        std::thread::sleep(Duration::from_millis(8));
    }
}
fn wait_pair(
    host: &mut Replica,
    guest: &mut Replica,
    condition: impl Fn(&Replica, &Replica) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(6);
    while !condition(host, guest) {
        host.pump();
        guest.pump();
        assert!(
            Instant::now() < deadline,
            "connection/match transition timed out"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
#[ignore = "requires Python 3.11+ and local sockets"]
fn default_ui_hosts_joins_and_spawns_a_real_multiplayer_round_without_bots() {
    use jetgame::ui::{Screen, UiController, UiKey};
    fn keys(ui: &mut UiController, keys: &[UiKey]) {
        for key in keys {
            ui.key(*key);
        }
    }
    let mut host = Replica::new(2);
    let mut guest = Replica::new(2);
    let mut ui = UiController::default();
    keys(&mut ui, &[UiKey::Enter, UiKey::Enter]);
    assert_eq!(ui.screen(), Screen::ServerBrowser);
    keys(
        &mut ui,
        &[UiKey::Down, UiKey::Down, UiKey::Down, UiKey::Enter],
    );
    assert_eq!(ui.screen(), Screen::Host);
    keys(
        &mut ui,
        &[
            UiKey::Down,
            UiKey::Down,
            UiKey::Down,
            UiKey::Down,
            UiKey::Enter,
        ],
    );
    assert_eq!(ui.screen(), Screen::Joining);
    host.ui = Some(ui);
    wait_pair(&mut host, &mut guest, |h, _| {
        h.connected && h.link.is_some()
    });
    assert_eq!(host.ui.as_ref().unwrap().screen(), Screen::Lobby);

    let mut ui = UiController::default();
    keys(&mut ui, &[UiKey::Enter, UiKey::Enter]);
    ui.connect_direct(host.link.clone().unwrap());
    guest.ui = Some(ui);
    wait_pair(&mut host, &mut guest, |_, g| g.connected);
    guest.ui.as_mut().unwrap().key(UiKey::Enter); // Ready
    wait_pair(&mut host, &mut guest, |h, g| h.ready && g.ready);
    keys(host.ui.as_mut().unwrap(), &[UiKey::Down, UiKey::Enter]); // Start
    wait_pair(&mut host, &mut guest, |h, g| {
        h.game.is_active() && g.game.is_active()
    });
    assert_eq!(host.world.rules.bot_count, 0);
    assert_eq!(host.ui.as_ref().unwrap().screen(), Screen::Spawn);
    assert_eq!(guest.ui.as_ref().unwrap().screen(), Screen::Spawn);
    keys(
        host.ui.as_mut().unwrap(),
        &[UiKey::Down, UiKey::Down, UiKey::Enter],
    );
    keys(
        guest.ui.as_mut().unwrap(),
        &[UiKey::Right, UiKey::Down, UiKey::Down, UiKey::Enter],
    );
    drive(&mut host, &mut guest, 45, &Default::default());
    assert_eq!(host.world.actors.iter().filter(|a| a.alive()).count(), 2);
    assert_eq!(guest.world.actors.iter().filter(|a| a.alive()).count(), 2);
    assert!(host.world.actors[1].alive());
    let position = host.world.actors[1].body.position;
    drive(&mut host, &mut guest, 75, &Default::default());
    assert!(host.world.actors[1].body.position.distance(position) > 100.);
    assert!(
        guest.world.actors[1]
            .body
            .position
            .distance(host.world.actors[1].body.position)
            < 100.
    );
    host.send(Request::Leave);
    wait_pair(&mut host, &mut guest, |_, g| g.disconnected);
    assert_eq!(guest.ui.as_ref().unwrap().screen(), Screen::ServerBrowser);
}

#[test]
#[ignore = "requires Python 3.11+ and local sockets"]
fn python_udp_gameplay_has_host_authority_prediction_late_join_and_restart() {
    use jetgame::{
        flight::{FlightInput, FlightKey},
        game::net_types::GameAction,
    };
    let mut host = Replica::new(1);
    let mut guest = Replica::new(2);
    host.send(Request::Host {
        op: 1,
        options: HostOptions {
            name: "SYNC HOST".into(),
            max_players: 4,
            map: "WORLD 01".into(),
            mode: "TEAM DOGFIGHT".into(),
        },
    });
    wait_pair(&mut host, &mut guest, |h, _| {
        h.connected && h.link.is_some()
    });
    let endpoint = host.endpoint.clone().unwrap();
    let link = host.link.clone().unwrap();
    guest.send(Request::Join {
        op: 2,
        endpoint: link.clone(),
    });
    wait_pair(&mut host, &mut guest, |_, g| g.connected);
    guest.send(Request::Ready { op: 2, ready: true });
    wait_pair(&mut host, &mut guest, |h, g| h.ready && g.ready);
    host.send(Request::Start { op: 1 });
    wait_pair(&mut host, &mut guest, |h, g| {
        h.game.is_active() && g.game.is_active()
    });
    assert_eq!(guest.world.local_actor, 1);
    host.action(1, GameAction::Spawn, 0);
    guest.action(1, GameAction::Spawn, 1);
    let mut input = FlightInput::default();
    drive(&mut host, &mut guest, 40, &input);
    assert!(host.world.actors[0].alive() && host.world.actors[1].alive());
    assert!(guest.world.actors[1].alive());
    let position = host.world.actors[1].body.position;
    input.set(FlightKey::D, true);
    input.set(FlightKey::Fire, true);
    drive(&mut host, &mut guest, 100, &input);
    assert!(host.world.actors[1].body.position.distance(position) > 100.);
    assert!(
        host.world.actors[1].stats.shots > 0,
        "guest fire must execute on the host"
    );
    assert!(
        guest.world.actors[1].gun_ammo < 800,
        "authoritative ammo must reach the guest"
    );
    assert!(
        guest.world.actors[1]
            .body
            .position
            .distance(host.world.actors[1].body.position)
            < 100.,
        "prediction must remain near authority"
    );
    host.world.actors[1].health = 64.3;
    drive(&mut host, &mut guest, 25, &input);
    assert!((guest.world.actors[1].health - 64.3).abs() < 0.11);
    guest.game.swap_weapon();
    drive(&mut host, &mut guest, 35, &input);
    assert!(host.world.actors[1].missiles < 8);
    assert!(guest.world.actors[1].missiles < 8);
    let mut late = Replica::new(3);
    late.send(Request::Join {
        op: 3,
        endpoint: link,
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !late.game.is_active() {
        host.step(&Default::default());
        guest.step(&input);
        late.pump();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(8));
    }
    assert_eq!(late.world.local_actor, 2);
    for _ in 0..30 {
        host.step(&Default::default());
        guest.step(&input);
        late.step(&Default::default());
        std::thread::sleep(Duration::from_millis(8));
    }
    assert!(
        late.world.actors[0].alive(),
        "late join must receive existing aircraft"
    );
    late.send(Request::Leave);
    let old_epoch = host.game.epoch().unwrap();
    host.send(Request::ReturnLobby {
        op: 1,
        epoch: host.game.epoch().unwrap(),
    });
    wait_pair(&mut host, &mut guest, |h, g| {
        !h.game.is_active() && !g.game.is_active()
    });
    guest.send(Request::Ready { op: 2, ready: true });
    wait_pair(&mut host, &mut guest, |h, g| h.ready && g.ready);
    host.send(Request::Start { op: 1 });
    wait_pair(&mut host, &mut guest, |h, g| {
        h.game.is_active() && g.game.is_active()
    });
    assert_ne!(host.game.epoch().unwrap(), old_epoch);
    guest.send(Request::Action {
        op: 2,
        epoch: old_epoch,
        action_id: 999,
        action: GameAction::Spawn,
        team: 1,
    });
    guest.send(Request::Inputs {
        op: 2,
        epoch: old_epoch,
        first: 1,
        frames: vec![[0, 0, 0, 550, 1, 0]; 6],
    });
    drive(&mut host, &mut guest, 10, &Default::default());
    assert!(
        !host.world.actors[1].alive(),
        "old-round actions must not spawn in the new round"
    );
    host.action(1, GameAction::End, 0);
    drive(&mut host, &mut guest, 20, &Default::default());
    assert!(host.world.finished && guest.world.finished);
    host.send(Request::Leave);
    wait_pair(&mut host, &mut guest, |_, g| g.disconnected);
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

#[test]
#[ignore = "requires bundled Python and local sockets"]
fn normal_launcher_configuration_hosts_with_bundled_python_in_both_profiles() {
    let mut config = NetworkConfig::load_default().unwrap();
    config.set("host_port", 0);
    config.set("lan_enabled", false);
    let mut client = NetworkClient::new(config);
    client
        .submit(Request::Host {
            op: 1,
            options: HostOptions {
                name: "LAUNCHER CHECK".into(),
                max_players: 8,
                map: "WORLD 01".into(),
                mode: "TEAM DOGFIGHT".into(),
            },
        })
        .unwrap();
    let event = wait(&mut client, |e| {
        matches!(
            e,
            NetworkEvent::Connected { .. } | NetworkEvent::WorkerStopped { .. }
        )
    });
    assert!(
        matches!(event, NetworkEvent::Connected { info, .. } if info.host && info.name == "LAUNCHER CHECK")
    );
    client.submit(Request::Leave).unwrap();
}
