//! Application-facing commands/events. All Internet/LAN protocols live in Python.

mod bridge;
mod config;

pub use bridge::NetworkClient;
pub use config::NetworkConfig;
use serde::{Deserialize, Serialize};

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
#[derive(Clone, Debug, Serialize)]
pub struct HostOptions {
    pub name: String,
    pub max_players: usize,
    pub map: String,
    pub mode: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkScope {
    Internet,
    Lan,
    #[default]
    Local,
}

#[derive(Debug, Serialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Discover { op: u64 },
    Host { op: u64, options: HostOptions },
    Join { op: u64, endpoint: String },
    Ready { op: u64, ready: bool },
    Start { op: u64 },
    Leave,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
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
    ShareLink {
        op: u64,
        link: String,
        scope: LinkScope,
        warning: String,
    },
    Error {
        op: u64,
        message: String,
    },
    Disconnected {
        op: u64,
        reason: String,
    },
    WorkerStopped {
        reason: String,
    },
}
