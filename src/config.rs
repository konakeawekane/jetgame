//! Editable game policy and asset addresses. Algorithms retain their own numerical constants.

use crate::flight::FlightTuning;
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GameConfig {
    pub title: String,
    pub window: [u32; 2],
    pub gameplay_fps: u32,
    pub menu_fps: u32,
    pub assets: AssetConfig,
    pub rules: MatchRules,
    pub flight: FlightTuning,
    pub sync: SyncTuning,
}
impl Default for GameConfig {
    fn default() -> Self {
        Self {
            title: "JetGame".into(),
            window: [1280, 720],
            gameplay_fps: 120,
            menu_fps: 30,
            assets: AssetConfig::default(),
            rules: MatchRules::default(),
            flight: FlightTuning::default(),
            sync: SyncTuning::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SyncTuning {
    pub input_hz: u32,
    pub snapshot_hz: u32,
    pub actor_chunk: usize,
    pub projectile_chunk: usize,
    pub redundancy_ticks: usize,
    pub input_buffer_ticks: usize,
    pub input_timeout: f32,
    pub interpolation_seconds: f32,
    pub extrapolation_seconds: f32,
    pub max_prediction_ticks: usize,
    pub max_input_queue: usize,
    pub max_packet_bytes: usize,
}
impl Default for SyncTuning {
    fn default() -> Self {
        Self {
            input_hz: 20,
            snapshot_hz: 20,
            actor_chunk: 4,
            projectile_chunk: 24,
            redundancy_ticks: 12,
            input_buffer_ticks: 3,
            input_timeout: 0.25,
            interpolation_seconds: 0.12,
            extrapolation_seconds: 0.75,
            max_prediction_ticks: 256,
            max_input_queue: 64,
            max_packet_bytes: 1200,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AssetConfig {
    pub root: PathBuf,
    pub aircraft: PathBuf,
    pub terrain: PathBuf,
    pub missile: PathBuf,
    pub bullet: PathBuf,
    pub terrain_width: f32,
    pub terrain_height_scale: f32,
}
impl Default for AssetConfig {
    fn default() -> Self {
        Self {
            root: "assets".into(),
            aircraft: "f-16_fighter_jet.glb".into(),
            terrain: "world_01.glb".into(),
            missile: "rocket.glb".into(),
            bullet: "bullet.glb".into(),
            terrain_width: 12000.,
            terrain_height_scale: 0.22,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MatchRules {
    pub physics_hz: u32,
    pub bot_count: usize,
    pub round_seconds: f32,
    pub kill_limit: u32,
    pub max_projectiles: usize,
    pub cover_count: usize,
    pub cover_seed: u32,
    pub health: f32,
    pub gun_ammo: u32,
    pub missiles: u32,
    pub gun_hz: f32,
    pub bot_gun_hz: f32,
    pub missile_cooldown: f32,
    pub gun_damage: f32,
    pub missile_damage: f32,
    pub radar_range: f32,
    pub radar_acquire_seconds: f32,
    pub radar_acquire_degrees: f32,
    pub radar_hold_degrees: f32,
    pub respawn_seconds: f32,
    pub spawn_protection_seconds: f32,
}
impl Default for MatchRules {
    fn default() -> Self {
        Self {
            physics_hz: 120,
            bot_count: 0,
            round_seconds: 180.,
            kill_limit: 5,
            max_projectiles: 256,
            cover_count: 72,
            cover_seed: 12345,
            health: 100.,
            gun_ammo: 800,
            missiles: 8,
            gun_hz: 12.,
            bot_gun_hz: 2.5,
            missile_cooldown: 0.9,
            gun_damage: 18.,
            missile_damage: 75.,
            radar_range: 2800.,
            radar_acquire_seconds: 1.25,
            radar_acquire_degrees: 30.,
            radar_hold_degrees: 45.,
            respawn_seconds: 5.,
            spawn_protection_seconds: 2.,
        }
    }
}
impl MatchRules {
    pub fn fixed_dt(self) -> f32 {
        1. / self.physics_hz as f32
    }
}

impl GameConfig {
    pub fn load(root: &Path, explicit: Option<&Path>) -> io::Result<Self> {
        let local = root.join("game.local.json");
        let path = explicit.map(Path::to_path_buf).unwrap_or_else(|| {
            if local.is_file() {
                local
            } else {
                root.join("game.json")
            }
        });
        let config: Self = if path.is_file() {
            let bytes = std::fs::read(path)?;
            serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        } else if explicit.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Game configuration file not found",
            ));
        } else {
            Self::default()
        };
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> io::Result<()> {
        let r = self.rules;
        let s = self.sync;
        let positive = [
            r.round_seconds,
            r.health,
            r.gun_hz,
            r.bot_gun_hz,
            r.missile_cooldown,
            r.gun_damage,
            r.missile_damage,
            r.radar_range,
            r.radar_acquire_seconds,
            r.respawn_seconds,
            self.assets.terrain_width,
            self.assets.terrain_height_scale,
            self.flight.mass,
            self.flight.wing_area,
            self.flight.thrust,
            self.flight.angular_response,
            self.flight.side_damping,
        ];
        if !(5..=30).contains(&s.input_hz)
            || !(5..=30).contains(&s.snapshot_hz)
            || !(1..=8).contains(&s.actor_chunk)
            || !(1..=32).contains(&s.projectile_chunk)
            || s.redundancy_ticks > 32
            || s.input_buffer_ticks > 12
            || !(64..=512).contains(&s.max_prediction_ticks)
            || !(16..=128).contains(&s.max_input_queue)
            || !(512..=1200).contains(&s.max_packet_bytes)
            || !(0.05..=0.5).contains(&s.input_timeout)
            || !(0.05..=0.3).contains(&s.interpolation_seconds)
            || !(0.1..=1.5).contains(&s.extrapolation_seconds)
            || r.physics_hz.div_ceil(s.input_hz.max(1)) as usize + s.redundancy_ticks > 64
            || r.gun_ammo > 1_000_000
            || r.missiles > 1_000_000
            || self.title.is_empty()
            || self.title.len() > 128
            || self.window.iter().any(|n| !(320..=16384).contains(n))
            || !(1..=1000).contains(&self.gameplay_fps)
            || !(1..=240).contains(&self.menu_fps)
            || !(30..=240).contains(&r.physics_hz)
            || r.bot_count > 15
            || r.max_projectiles == 0
            || r.max_projectiles > 8192
            || r.cover_count > 4096
            || r.kill_limit == 0
            || r.health > 255.
            || positive.iter().any(|n| !n.is_finite() || *n <= 0.)
            || !r.spawn_protection_seconds.is_finite()
            || r.spawn_protection_seconds < 0.
            || !(1. ..=89.).contains(&r.radar_acquire_degrees)
            || !(r.radar_acquire_degrees..=120.).contains(&r.radar_hold_degrees)
            || !self.flight.inertia.is_finite()
            || self.flight.inertia.min_element() <= 0.
            || !self.flight.max_rates.is_finite()
            || self.flight.max_rates.min_element() <= 0.
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Game configuration contains an invalid value",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_rules_fail_before_simulation() {
        let mut config = GameConfig::default();
        config.validate().unwrap();
        config.rules.physics_hz = 0;
        assert!(config.validate().is_err());
        config.rules.physics_hz = 60;
        config.flight.mass = f32::NAN;
        assert!(config.validate().is_err());
    }
}
