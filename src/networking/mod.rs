//! Native Rust client/control worker. Python is used only for headless hosting.

mod hosting;
pub mod protocol;

use hosting::HostedServer;
use protocol::{Decoder, Message, encode, exchange, invalid};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub directory: Option<String>,
    pub python: Option<PathBuf>,
    pub server_script: Option<PathBuf>,
    pub host_bind: String,
    pub host_port: u16,
    pub public_port: Option<u16>,
    #[serde(skip)]
    pub root: PathBuf,
}
impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            directory: None,
            python: None,
            server_script: None,
            host_bind: "0.0.0.0".into(),
            host_port: 29010,
            public_port: None,
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}
impl NetworkConfig {
    pub fn load_default() -> io::Result<Self> {
        let executable = std::env::current_exe()?;
        let parent = executable.parent().unwrap();
        let root = if parent.join("networking/server_main.py").is_file() {
            parent.to_path_buf()
        } else {
            std::env::current_dir()?
        };
        let local = root.join("networking.local.json");
        let path = if local.is_file() {
            local
        } else {
            root.join("networking.json")
        };
        let mut config = if path.is_file() {
            Self::from_json(&std::fs::read(path)?)?
        } else {
            Self::default()
        };
        config.root = root;
        if let Ok(endpoint) = std::env::var("JETGAME_DIRECTORY") {
            config.directory = Some(endpoint);
        }
        if let Some(python) = std::env::var_os("JETGAME_SERVER_PYTHON") {
            config.python = Some(python.into());
        }
        Ok(config)
    }
    fn from_json(bytes: &[u8]) -> io::Result<Self> {
        // Windows PowerShell may write UTF-8 configuration files with a BOM.
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
            .map_err(invalid)
    }
    fn resolve(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.into()
        } else {
            self.root.join(path)
        }
    }
    fn python_executable(&self) -> io::Result<PathBuf> {
        if let Some(path) = &self.python {
            return Ok(self.resolve(path));
        }
        let bundled = self.root.join(if cfg!(windows) {
            "runtime/python/python.exe"
        } else {
            "runtime/python/bin/python3"
        });
        if bundled.is_file() {
            return Ok(bundled);
        }
        if cfg!(debug_assertions) {
            return Ok(PathBuf::from(if cfg!(windows) {
                "python"
            } else {
                "python3"
            }));
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Hosting Python runtime is missing; configure --server-python or package runtime/python",
        ))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerListing {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub players: usize,
    pub max_players: usize,
    pub in_progress: bool,
    pub reachability: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: u64,
    pub name: String,
    pub ready: bool,
    pub host: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionInfo {
    pub name: String,
    pub player_id: u64,
    pub host: bool,
    pub max_players: usize,
    pub map: String,
    pub mode: String,
    pub capabilities: Vec<String>,
    pub players: Vec<PeerInfo>,
}
#[derive(Clone, Debug)]
pub struct HostOptions {
    pub name: String,
    pub max_players: usize,
    pub map: String,
    pub mode: String,
}

pub enum Request {
    Discover { op: u64 },
    Host { op: u64, options: HostOptions },
    Join { op: u64, endpoint: String },
    Ready { op: u64, ready: bool },
    Start { op: u64 },
    Leave,
}
#[derive(Debug)]
pub enum NetworkEvent {
    Servers {
        op: u64,
        servers: Vec<ServerListing>,
    },
    Connected {
        op: u64,
        info: SessionInfo,
        endpoint: String,
    },
    Lobby {
        op: u64,
        players: Vec<PeerInfo>,
    },
    Error {
        op: u64,
        message: String,
    },
    Disconnected {
        op: u64,
        reason: String,
    },
    WorkerStopped,
}

pub struct NetworkClient {
    requests: SyncSender<Request>,
    events: Receiver<NetworkEvent>,
    stop: Arc<AtomicBool>,
    reported_stop: bool,
}
impl NetworkClient {
    pub fn new(config: NetworkConfig) -> Self {
        let (tx, rx) = mpsc::sync_channel(16);
        let (events, out) = mpsc::sync_channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        std::thread::spawn(move || worker(config, rx, events, worker_stop));
        Self {
            requests: tx,
            events: out,
            stop,
            reported_stop: false,
        }
    }
    pub fn submit(&self, request: Request) -> Result<(), String> {
        self.requests
            .try_send(request)
            .map_err(|_| "Networking request queue is unavailable or full".into())
    }
    pub fn poll(&mut self) -> Vec<NetworkEvent> {
        let mut events = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.reported_stop {
                        events.push(NetworkEvent::WorkerStopped);
                        self.reported_stop = true;
                    }
                    break;
                }
            }
        }
        events
    }
}
impl Drop for NetworkClient {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn connect(endpoint: &str) -> io::Result<TcpStream> {
    let mut last = invalid("endpoint has no resolved addresses");
    for address in endpoint.to_socket_addrs()?.take(4) {
        match TcpStream::connect_timeout(&address, Duration::from_secs(2)) {
            Ok(stream) => {
                stream.set_nodelay(true)?;
                return Ok(stream);
            }
            Err(error) => last = error,
        }
    }
    Err(last)
}

pub fn discover(endpoint: &str) -> io::Result<Vec<ServerListing>> {
    let mut servers = Vec::new();
    let mut offset = 0;
    loop {
        let mut stream = connect(endpoint)?;
        let reply = exchange(
            &mut stream,
            &Message::new("discover", json!({"offset": offset})),
        )?;
        if reply.kind != "servers" {
            return Err(invalid(
                reply.payload["message"]
                    .as_str()
                    .unwrap_or("directory request failed"),
            ));
        }
        let page: Vec<ServerListing> =
            serde_json::from_value(reply.payload["servers"].clone()).map_err(invalid)?;
        if page.len() > 100
            || servers.len() + page.len() > 512
            || page
                .iter()
                .any(|s| s.name.len() > 128 || s.endpoint.len() > 255 || s.max_players > 32)
        {
            return Err(invalid("invalid directory listing"));
        }
        servers.extend(page);
        match reply.payload.get("next_offset") {
            None | Some(Value::Null) => return Ok(servers),
            Some(value) => {
                let next = value
                    .as_u64()
                    .ok_or_else(|| invalid("invalid directory cursor"))?;
                if next != offset + 100 || next >= 512 {
                    return Err(invalid("invalid directory cursor"));
                }
                offset = next;
            }
        }
    }
}

struct Connection {
    op: u64,
    stream: TcpStream,
    decoder: Decoder,
    outgoing: VecDeque<Vec<u8>>,
    offset: usize,
    last_received: Instant,
    last_ping: Instant,
}
impl Connection {
    fn join(op: u64, endpoint: &str, token: Option<&str>) -> io::Result<(Self, SessionInfo)> {
        let mut stream = connect(endpoint)?;
        let reply = exchange(
            &mut stream,
            &Message::new(
                "join",
                json!({"name":"PILOT","admin_token":token.unwrap_or("")}),
            ),
        )?;
        if reply.kind != "welcome" {
            return Err(invalid(
                reply.payload["message"].as_str().unwrap_or("join failed"),
            ));
        }
        let info: SessionInfo = serde_json::from_value(reply.payload).map_err(invalid)?;
        if info.players.len() > 32
            || info.name.len() > 128
            || !info.capabilities.iter().any(|c| c == "lobby")
        {
            return Err(invalid("unsupported server capabilities"));
        }
        stream.set_read_timeout(None)?;
        stream.set_write_timeout(None)?;
        stream.set_nonblocking(true)?;
        Ok((
            Self {
                op,
                stream,
                decoder: Decoder::default(),
                outgoing: VecDeque::new(),
                offset: 0,
                last_received: Instant::now(),
                last_ping: Instant::now(),
            },
            info,
        ))
    }
    fn queue(&mut self, kind: &str, payload: Value) -> io::Result<()> {
        if self.outgoing.len() >= 32 {
            return Err(invalid("send queue limit exceeded"));
        }
        self.outgoing
            .push_back(encode(&Message::new(kind, payload))?);
        Ok(())
    }
    fn pump(&mut self) -> io::Result<Vec<NetworkEvent>> {
        if self.last_received.elapsed() > Duration::from_secs(8) {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "server timed out"));
        }
        if self.last_ping.elapsed() > Duration::from_secs(2) {
            self.queue("ping", json!({}))?;
            self.last_ping = Instant::now();
        }
        while let Some(frame) = self.outgoing.front() {
            match self.stream.write(&frame[self.offset..]) {
                Ok(0) => return Err(invalid("connection closed")),
                Ok(count) => {
                    self.offset += count;
                    if self.offset == frame.len() {
                        self.outgoing.pop_front();
                        self.offset = 0;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        let mut buffer = [0; 4096];
        for _ in 0..16 {
            match self.stream.read(&mut buffer) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "server disconnected",
                    ));
                }
                Ok(count) => self.decoder.push(&buffer[..count])?,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        let mut events = Vec::new();
        for _ in 0..16 {
            let Some(message) = self.decoder.pop()? else {
                break;
            };
            self.last_received = Instant::now();
            match message.kind.as_str() {
                "pong" => {}
                "lobby" => {
                    let players: Vec<PeerInfo> =
                        serde_json::from_value(message.payload["players"].clone())
                            .map_err(invalid)?;
                    if players.len() > 32 {
                        return Err(invalid("roster too large"));
                    }
                    events.push(NetworkEvent::Lobby {
                        op: self.op,
                        players,
                    });
                }
                "error" => events.push(NetworkEvent::Error {
                    op: self.op,
                    message: message.payload["message"]
                        .as_str()
                        .unwrap_or("server error")
                        .chars()
                        .take(160)
                        .collect(),
                }),
                _ => return Err(invalid("server sent an unsupported control message")),
            }
        }
        Ok(events)
    }
}

fn worker(
    config: NetworkConfig,
    requests: Receiver<Request>,
    events: SyncSender<NetworkEvent>,
    stop: Arc<AtomicBool>,
) {
    let mut connection: Option<Connection> = None;
    let mut hosted: Option<HostedServer> = None;
    let emit = |event| events.try_send(event).is_ok();
    while !stop.load(Ordering::Acquire) {
        match requests.recv_timeout(Duration::from_millis(20)) {
            Ok(Request::Discover { op }) => {
                let result = config
                    .directory
                    .as_deref()
                    .ok_or_else(|| invalid("Public directory is not configured"))
                    .and_then(discover);
                let event = match result {
                    Ok(servers) => NetworkEvent::Servers { op, servers },
                    Err(error) => NetworkEvent::Error {
                        op,
                        message: error.to_string(),
                    },
                };
                if !emit(event) {
                    break;
                }
            }
            Ok(Request::Host { op, options }) => {
                connection = None;
                hosted = None;
                match HostedServer::start(&config, &options) {
                    Ok(server) => {
                        let result =
                            Connection::join(op, &server.endpoint, Some(&server.admin_token));
                        match result {
                            Ok((client, info)) => {
                                let endpoint = server.endpoint.clone();
                                connection = Some(client);
                                hosted = Some(server);
                                if !emit(NetworkEvent::Connected { op, info, endpoint }) {
                                    break;
                                }
                            }
                            Err(error) => {
                                if !emit(NetworkEvent::Error {
                                    op,
                                    message: error.to_string(),
                                }) {
                                    break;
                                }
                            }
                        }
                    }
                    Err(error) => {
                        if !emit(NetworkEvent::Error {
                            op,
                            message: error.to_string(),
                        }) {
                            break;
                        }
                    }
                }
            }
            Ok(Request::Join { op, endpoint }) => {
                connection = None;
                hosted = None;
                match Connection::join(op, &endpoint, None) {
                    Ok((client, info)) => {
                        connection = Some(client);
                        if !emit(NetworkEvent::Connected { op, info, endpoint }) {
                            break;
                        }
                    }
                    Err(error) => {
                        if !emit(NetworkEvent::Error {
                            op,
                            message: error.to_string(),
                        }) {
                            break;
                        }
                    }
                }
            }
            Ok(Request::Leave) => {
                connection = None;
                hosted = None;
            }
            Ok(request) => {
                if let Some(client) = &mut connection {
                    let result = match request {
                        Request::Ready { op, ready } if op == client.op => {
                            client.queue("ready", json!({"ready":ready}))
                        }
                        Request::Start { op } if op == client.op => {
                            client.queue("start_match", json!({}))
                        }
                        _ => Ok(()),
                    };
                    if let Err(error) = result {
                        if !emit(NetworkEvent::Error {
                            op: client.op,
                            message: error.to_string(),
                        }) {
                            break;
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if let Some(client) = &mut connection {
            match client.pump() {
                Ok(updates) => {
                    if updates.into_iter().any(|event| !emit(event)) {
                        break;
                    }
                }
                Err(error) => {
                    let op = client.op;
                    connection = None;
                    hosted = None;
                    if !emit(NetworkEvent::Disconnected {
                        op,
                        reason: error.to_string(),
                    }) {
                        break;
                    }
                }
            }
        }
    }
    drop(connection);
    drop(hosted);
}

#[cfg(test)]
mod config_tests {
    use super::NetworkConfig;
    #[test]
    fn accepts_windows_utf8_configuration() {
        let config = NetworkConfig::from_json(b"\xef\xbb\xbf{\"host_port\":29012}").unwrap();
        assert_eq!(config.host_port, 29012);
    }
}
