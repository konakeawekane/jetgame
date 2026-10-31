//! Application-facing commands/events. All Internet/LAN protocols live in Python.

mod bridge;
mod config;

use crate::game::net_types::{
    GameAction, InputFrame, MatchSetup, PlayerSlot, StatePacket, WorldSnapshot,
};
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
    Discover {
        op: u64,
    },
    Host {
        op: u64,
        options: HostOptions,
    },
    Join {
        op: u64,
        endpoint: String,
    },
    Ready {
        op: u64,
        ready: bool,
    },
    Start {
        op: u64,
    },
    Begin {
        op: u64,
        setup: MatchSetup,
    },
    Inputs {
        op: u64,
        epoch: u32,
        first: u32,
        frames: Vec<InputFrame>,
    },
    State {
        op: u64,
        epoch: u32,
        snapshot: WorldSnapshot,
    },
    Action {
        op: u64,
        epoch: u32,
        action_id: u32,
        action: GameAction,
        team: u8,
    },
    ActionResult {
        op: u64,
        epoch: u32,
        player: u64,
        action_id: u32,
        accepted: bool,
        reason: String,
    },
    ReturnLobby {
        op: u64,
        epoch: u32,
    },
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
    Prepare {
        op: u64,
        epoch: u32,
        players: Vec<PlayerSlot>,
        capacity: usize,
        free_for_all: bool,
    },
    GameStarted {
        op: u64,
        setup: MatchSetup,
        players: Vec<PlayerSlot>,
    },
    Participants {
        op: u64,
        epoch: u32,
        players: Vec<PlayerSlot>,
    },
    Inputs {
        op: u64,
        epoch: u32,
        player: u64,
        first: u32,
        frames: Vec<InputFrame>,
    },
    State {
        op: u64,
        epoch: u32,
        packet: StatePacket,
    },
    Action {
        op: u64,
        epoch: u32,
        player: u64,
        action_id: u32,
        action: GameAction,
        team: u8,
    },
    ActionResult {
        op: u64,
        epoch: u32,
        action_id: u32,
        accepted: bool,
        reason: String,
    },
    ReturnedLobby {
        op: u64,
    },
}

impl NetworkEvent {
    pub fn operation(&self) -> Option<u64> {
        match self {
            Self::Servers { op, .. }
            | Self::Connected { op, .. }
            | Self::Lobby { op, .. }
            | Self::ShareLink { op, .. }
            | Self::Error { op, .. }
            | Self::Disconnected { op, .. }
            | Self::Prepare { op, .. }
            | Self::GameStarted { op, .. }
            | Self::Participants { op, .. }
            | Self::Inputs { op, .. }
            | Self::State { op, .. }
            | Self::Action { op, .. }
            | Self::ActionResult { op, .. }
            | Self::ReturnedLobby { op } => Some(*op),
            Self::WorkerStopped { .. } => None,
        }
    }
}
