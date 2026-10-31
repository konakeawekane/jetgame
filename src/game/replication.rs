//! Authoritative world export and bounded replica import. No transport or sockets.

use super::net_types::*;
use super::*;

fn q(value: f32, scale: f32) -> i64 {
    (value * scale).round() as i64
}
fn v(row: &[i64], scale: f32) -> Vec3 {
    Vec3::new(row[0] as f32, row[1] as f32, row[2] as f32) / scale
}

impl World {
    pub(super) fn enemies(&self, a: usize, b: usize) -> bool {
        if a == b {
            return false;
        }
        if !self.online {
            return a == 0 || b == 0;
        }
        self.free_for_all || self.actors[a].team != self.actors[b].team
    }
    pub(super) fn bot_target(&self, source: usize) -> Option<usize> {
        self.actors
            .iter()
            .enumerate()
            .filter(|(i, a)| a.alive() && self.enemies(source, *i))
            .min_by(|(_, a), (_, b)| {
                a.body
                    .position
                    .distance_squared(self.actors[source].body.position)
                    .total_cmp(
                        &b.body
                            .position
                            .distance_squared(self.actors[source].body.position),
                    )
            })
            .map(|(i, _)| i)
    }
    pub(super) fn update_local_stats(&mut self) {
        let s = self.actors[self.local_actor].stats;
        (self.kills, self.deaths, self.shots, self.hits) = (s.kills, s.deaths, s.shots, s.hits);
    }
    pub(super) fn network_spawn_point(&self, slot: usize, team: u8) -> (Vec3, Quat) {
        let forward = if team == 0 { -Vec3::Z } else { Vec3::Z };
        let x = (slot % 8) as f32 * 350. - 1225.;
        let z = if team == 0 { 3500. } else { -3500. } + forward.z * (slot / 8) as f32 * 400.;
        let position = Vec3::new(
            x,
            self.terrain.ground_at(x, z) + 450. + (slot % 3) as f32 * 40.,
            z,
        );
        (
            position,
            if team == 0 {
                Quat::IDENTITY
            } else {
                Quat::from_rotation_y(std::f32::consts::PI)
            },
        )
    }
    pub(super) fn configure_network(&mut self, setup: &MatchSetup, local: usize, authority: bool) {
        self.online = true;
        self.human_slots = setup.capacity;
        self.local_actor = local;
        self.free_for_all = setup.free_for_all;
        self.rules = setup.rules;
        self.tuning = setup.flight;
        self.remote_audio = if authority { None } else { Some(0) };
        self.visual_offset = Vec3::ZERO;
        self.visual_rotation = Quat::IDENTITY;
        self.actors.clear();
        self.projectiles.clear();
        self.explosions.clear();
        self.cover.clear();
        self.next_projectile = 1;
        self.next_explosion = 1;
        self.elapsed = 0.;
        self.finished = false;
        self.accumulator = 0.;
        self.generate_cover();
        for slot in 0..setup.capacity + setup.rules.bot_count {
            let team = (slot % 2) as u8;
            let (p, r) = self.network_spawn_point(slot, team);
            let mut actor = Aircraft::new(p, r, 1, self.rules);
            actor.team = team;
            if slot < setup.capacity || !authority {
                actor.health = 0.;
            }
            self.actors.push(actor);
        }
        self.update_local_stats();
    }
    pub(super) fn spawn_network(&mut self, slot: usize, team: u8) -> Result<(), String> {
        if slot >= self.human_slots || team > 1 || self.finished {
            return Err("Spawn is unavailable".into());
        }
        let actor = &self.actors[slot];
        if actor.alive() {
            return Err("Aircraft is already flying".into());
        }
        if actor.respawn > 0. {
            return Err(format!("Respawn available in {:.0}s", actor.respawn.ceil()));
        }
        let (p, r) = self.network_spawn_point(slot, team);
        let (life, stats) = (actor.life.wrapping_add(1).max(1), actor.stats);
        let mut actor = Aircraft::new(p, r, life, self.rules);
        actor.team = team;
        actor.stats = stats;
        self.actors[slot] = actor;
        if slot == self.local_actor {
            self.visual_offset = Vec3::ZERO;
            self.visual_rotation = Quat::IDENTITY;
        }
        Ok(())
    }
    pub(super) fn deactivate(&mut self, slot: usize) {
        let actor = &mut self.actors[slot];
        actor.health = 0.;
        actor.life = actor.life.wrapping_add(1).max(1);
        actor.respawn = 0.;
        actor.radar = Radar::default();
        actor.stats = AircraftStats::default();
        self.projectiles.retain(|p| p.owner != slot);
    }
    pub fn snapshot(&self, tick: u64, acks: Vec<(u64, u32)>) -> WorldSnapshot {
        let actors = self
            .actors
            .iter()
            .enumerate()
            .map(|(slot, a)| {
                let p = a.body.position;
                let v = a.body.velocity;
                let r = a.body.orientation;
                let w = a.body.angular_velocity;
                let target = a.radar.target;
                [
                    slot as i64,
                    a.life as i64,
                    a.team as i64,
                    q(a.health, 10.),
                    a.gun_ammo as i64,
                    a.missiles as i64,
                    i64::from(a.weapon == Weapon::Missiles),
                    q(a.cooldown, 1000.),
                    q(a.respawn.max(0.), 1000.),
                    q(a.invulnerable, 1000.),
                    target.map_or(-1, |t| t.actor as i64),
                    target.map_or(0, |t| t.life as i64),
                    q(a.radar.progress, 1000.),
                    i64::from(a.radar.locked),
                    a.stats.kills as i64,
                    a.stats.deaths as i64,
                    a.stats.shots as i64,
                    a.stats.hits as i64,
                    q(p.x, 100.),
                    q(p.y, 100.),
                    q(p.z, 100.),
                    q(v.x, 100.),
                    q(v.y, 100.),
                    q(v.z, 100.),
                    q(r.x, 32767.),
                    q(r.y, 32767.),
                    q(r.z, 32767.),
                    q(r.w, 32767.),
                    q(w.x, 1000.),
                    q(w.y, 1000.),
                    q(w.z, 1000.),
                    self.audio_flags(slot) as i64,
                ]
            })
            .collect();
        let projectiles = self
            .projectiles
            .iter()
            .map(|p| {
                [
                    p.id as i64,
                    p.owner as i64,
                    i64::from(p.missile),
                    q(p.position.x, 100.),
                    q(p.position.y, 100.),
                    q(p.position.z, 100.),
                    q(p.velocity.x, 100.),
                    q(p.velocity.y, 100.),
                    q(p.velocity.z, 100.),
                    q(p.remaining.max(0.), 1000.),
                    p.target.map_or(-1, |t| t.actor as i64),
                    p.target.map_or(0, |t| t.life as i64),
                ]
            })
            .collect();
        let explosions = self
            .explosions
            .iter()
            .map(|e| {
                [
                    e.id as i64,
                    q(e.position.x, 100.),
                    q(e.position.y, 100.),
                    q(e.position.z, 100.),
                    q(e.age, 1000.),
                    q(e.radius, 100.),
                ]
            })
            .collect();
        WorldSnapshot {
            tick,
            elapsed_ms: q(self.elapsed, 1000.) as u32,
            finished: self.finished,
            actors,
            projectiles,
            explosions,
            acks,
        }
    }
    pub(super) fn import_actor(&mut self, row: &ActorRow) -> Result<(usize, RigidBody), String> {
        let slot = usize::try_from(row[0]).map_err(|_| "Invalid actor slot")?;
        if slot >= self.actors.len()
            || row[1] < 1
            || row[1] > u32::MAX as i64
            || !(0..=1).contains(&row[2])
            || !(0..=2550).contains(&row[3])
            || row[4..6].iter().any(|n| !(0..=1_000_000).contains(n))
            || !(0..=1).contains(&row[6])
            || row[7..10].iter().any(|n| !(0..=600_000).contains(n))
            || !(-1..self.actors.len() as i64).contains(&row[10])
            || !(0..=u32::MAX as i64).contains(&row[11])
            || !(0..=1000).contains(&row[12])
            || !(0..=1).contains(&row[13])
            || row[14..18]
                .iter()
                .any(|n| !(0..=u32::MAX as i64).contains(n))
            || row[18..24]
                .iter()
                .any(|n| !(-2_000_000_000..=2_000_000_000).contains(n))
            || row[24..28].iter().any(|n| !(-32767..=32767).contains(n))
            || row[28..31]
                .iter()
                .any(|n| !(-100_000..=100_000).contains(n))
            || !(0..=7).contains(&row[31])
        {
            return Err("Invalid aircraft state".into());
        }
        let rotation = Quat::from_xyzw(
            row[24] as f32 / 32767.,
            row[25] as f32 / 32767.,
            row[26] as f32 / 32767.,
            row[27] as f32 / 32767.,
        );
        if !(0.9..=1.1).contains(&rotation.length_squared()) {
            return Err("Invalid aircraft rotation".into());
        }
        let body = RigidBody {
            position: v(&row[18..21], 100.),
            velocity: v(&row[21..24], 100.),
            orientation: rotation.normalize(),
            angular_velocity: v(&row[28..31], 1000.),
        };
        let a = &mut self.actors[slot];
        a.life = row[1] as u32;
        a.team = row[2] as u8;
        a.health = row[3] as f32 / 10.;
        a.gun_ammo = row[4] as u32;
        a.missiles = row[5] as u32;
        a.weapon = if row[6] == 0 {
            Weapon::Guns
        } else {
            Weapon::Missiles
        };
        a.cooldown = row[7] as f32 / 1000.;
        a.respawn = row[8] as f32 / 1000.;
        a.invulnerable = row[9] as f32 / 1000.;
        a.radar = Radar {
            target: if row[10] < 0 {
                None
            } else {
                Some(Target {
                    actor: row[10] as usize,
                    life: row[11] as u32,
                })
            },
            progress: row[12] as f32 / 1000.,
            locked: row[13] != 0,
        };
        a.stats = AircraftStats {
            kills: row[14] as u32,
            deaths: row[15] as u32,
            shots: row[16] as u32,
            hits: row[17] as u32,
        };
        if slot == self.local_actor {
            self.remote_audio = Some(row[31] as u8);
        }
        Ok((slot, body))
    }
    pub(super) fn import_projectile(&mut self, row: &ProjectileRow) -> Result<(), String> {
        if row[0] < 1
            || row[0] > u32::MAX as i64
            || !(0..self.actors.len() as i64).contains(&row[1])
            || !(0..=1).contains(&row[2])
            || row[3..9]
                .iter()
                .any(|n| !(-2_000_000_000..=2_000_000_000).contains(n))
            || !(0..=10_000).contains(&row[9])
            || !(-1..self.actors.len() as i64).contains(&row[10])
            || !(0..=u32::MAX as i64).contains(&row[11])
        {
            return Err("Invalid projectile state".into());
        }
        let position = v(&row[3..6], 100.);
        let velocity = v(&row[6..9], 100.);
        let p = Projectile {
            id: row[0] as u32,
            position,
            velocity,
            previous_position: position,
            previous_velocity: velocity,
            owner: row[1] as usize,
            missile: row[2] != 0,
            remaining: row[9] as f32 / 1000.,
            target: if row[10] < 0 {
                None
            } else {
                Some(Target {
                    actor: row[10] as usize,
                    life: row[11] as u32,
                })
            },
        };
        if let Some(existing) = self.projectiles.iter_mut().find(|p| p.id == row[0] as u32) {
            *existing = p;
        } else if self.projectiles.len() < MAX_NETWORK_PROJECTILES {
            self.projectiles.push(p);
        }
        Ok(())
    }
    pub(super) fn import_explosion(&mut self, row: &ExplosionRow) -> Result<(), String> {
        if row[0] < 1
            || row[0] > u32::MAX as i64
            || row[1..4]
                .iter()
                .any(|n| !(-2_000_000_000..=2_000_000_000).contains(n))
            || !(0..=700).contains(&row[4])
            || !(0..=100_000).contains(&row[5])
        {
            return Err("Invalid explosion state".into());
        }
        let e = Explosion {
            id: row[0] as u32,
            position: v(&row[1..4], 100.),
            age: row[4] as f32 / 1000.,
            radius: row[5] as f32 / 100.,
        };
        if let Some(existing) = self.explosions.iter_mut().find(|e| e.id == row[0] as u32) {
            *existing = e;
        } else if self.explosions.len() < 48 {
            self.explosions.push(e);
        }
        Ok(())
    }
}
