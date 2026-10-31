//! Simulation messages. Python owns their wire encoding and all sockets.

use crate::{
    config::{MatchRules, SyncTuning},
    flight::FlightTuning,
};
use serde::{Deserialize, Serialize};

pub const MAX_ACTORS: usize = 47;
pub const MAX_NETWORK_PROJECTILES: usize = 256;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerSlot {
    pub id: u64,
    pub slot: usize,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatchSetup {
    pub epoch: u32,
    pub capacity: usize,
    pub rules: MatchRules,
    pub flight: FlightTuning,
    pub sync: SyncTuning,
    pub level_signature: u64,
    pub free_for_all: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameAction {
    Spawn,
    Respawn,
    End,
}

/// Axes are quantized to thousandths; fire and desired weapon are absolute states.
pub type InputFrame = [i16; 6];
/// Compact fixed-layout integer rows; distances/velocities use centimetres.
pub type ActorRow = [i64; 32];
pub type ProjectileRow = [i64; 12];
pub type ExplosionRow = [i64; 6];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorldSnapshot {
    pub tick: u64,
    pub elapsed_ms: u32,
    pub finished: bool,
    pub actors: Vec<ActorRow>,
    pub projectiles: Vec<ProjectileRow>,
    pub explosions: Vec<ExplosionRow>,
    pub acks: Vec<(u64, u32)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatePacket {
    #[serde(rename = "t")]
    pub tick: u64,
    #[serde(rename = "e")]
    pub elapsed_ms: u32,
    #[serde(rename = "f")]
    pub finished: bool,
    #[serde(rename = "k")]
    pub ack: u32,
    #[serde(rename = "a")]
    pub actors: Vec<ActorRow>,
    #[serde(rename = "p")]
    pub projectiles: Vec<ProjectileRow>,
    #[serde(rename = "x")]
    pub explosions: Vec<ExplosionRow>,
    #[serde(rename = "r")]
    pub removed: Vec<u32>,
}
