//! Offline flight/combat simulation. GPU meshes are shared; physics uses small local types.

use crate::config::{GameConfig, MatchRules};
use crate::{
    flight::{Controls, FlightInput, FlightTuning, RigidBody, steer_toward},
    terrain::{Terrain, segment_sphere},
};
use glam::{Mat4, Quat, Vec3};

mod presentation;
pub use presentation::{GameAssets, aim_direction, chase_camera};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Weapon {
    #[default]
    Guns,
    Missiles,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub actor: usize,
    pub life: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Radar {
    pub target: Option<Target>,
    pub progress: f32,
    pub locked: bool,
}

impl Radar {
    pub fn update(&mut self, target: Option<Target>, dt: f32) {
        self.update_with_duration(target, dt, MatchRules::default().radar_acquire_seconds);
    }
    fn update_with_duration(&mut self, target: Option<Target>, dt: f32, acquire_seconds: f32) {
        if target != self.target {
            self.target = target;
            self.progress = 0.;
            self.locked = false;
        }
        if target.is_some() {
            self.progress = (self.progress + dt / acquire_seconds).min(1.);
            self.locked = self.progress >= 1.;
        } else {
            self.progress = 0.;
            self.locked = false;
        }
    }
}

pub struct Aircraft {
    pub body: RigidBody,
    previous_body: RigidBody,
    pub health: f32,
    pub life: u32,
    pub radar: Radar,
    pub weapon: Weapon,
    pub gun_ammo: u32,
    pub missiles: u32,
    cooldown: f32,
    invulnerable: f32,
    respawn: f32,
    outside: f32,
}

impl Aircraft {
    fn new(position: Vec3, orientation: Quat, life: u32, rules: MatchRules) -> Self {
        Self {
            body: RigidBody::new(position, orientation),
            previous_body: RigidBody::new(position, orientation),
            health: rules.health,
            life,
            radar: Radar::default(),
            weapon: Weapon::Guns,
            gun_ammo: rules.gun_ammo,
            missiles: rules.missiles,
            cooldown: 0.,
            invulnerable: rules.spawn_protection_seconds,
            respawn: 0.,
            outside: 0.,
        }
    }
    pub fn alive(&self) -> bool {
        self.health > 0.
    }
}

pub struct Projectile {
    pub position: Vec3,
    pub velocity: Vec3,
    previous_position: Vec3,
    previous_velocity: Vec3,
    pub owner: usize,
    pub target: Option<Target>,
    pub missile: bool,
    remaining: f32,
}
pub struct Explosion {
    pub position: Vec3,
    pub age: f32,
    pub radius: f32,
}
pub struct Cover {
    pub transform: Mat4,
    pub center: Vec3,
    pub radius: f32,
}

pub struct World {
    pub terrain: Terrain,
    pub actors: Vec<Aircraft>,
    pub projectiles: Vec<Projectile>,
    pub explosions: Vec<Explosion>,
    pub cover: Vec<Cover>,
    pub kills: u32,
    pub deaths: u32,
    pub shots: u32,
    pub hits: u32,
    pub elapsed: f32,
    pub finished: bool,
    pub tuning: FlightTuning,
    pub rules: MatchRules,
    pub last_controls: Controls,
    accumulator: f64,
    team: usize,
}

impl World {
    /// One physics tick of visual latency keeps interpolation independent of render rate.
    pub fn render_alpha(&self) -> f32 {
        (self.accumulator / self.rules.fixed_dt() as f64).clamp(0., 1.) as f32
    }

    pub fn render_body(&self, actor: usize) -> RigidBody {
        let actor = &self.actors[actor];
        actor
            .previous_body
            .interpolate(actor.body, self.render_alpha())
    }

    /// Spawn/teleport discontinuities must never blend from an unrelated old position.
    pub fn snap_render_state(&mut self) {
        for actor in &mut self.actors {
            actor.previous_body = actor.body;
        }
        for p in &mut self.projectiles {
            p.previous_position = p.position;
            p.previous_velocity = p.velocity;
        }
        self.accumulator = 0.;
    }

    pub fn new(terrain: Terrain) -> Self {
        Self::configured(terrain, &GameConfig::default())
    }
    pub fn configured(terrain: Terrain, config: &GameConfig) -> Self {
        let rules = config.rules;
        let mut world = Self {
            terrain,
            actors: Vec::with_capacity(rules.bot_count + 1),
            projectiles: Vec::with_capacity(rules.max_projectiles),
            explosions: Vec::with_capacity(32),
            cover: Vec::with_capacity(rules.cover_count),
            kills: 0,
            deaths: 0,
            shots: 0,
            hits: 0,
            elapsed: 0.,
            finished: false,
            tuning: config.flight,
            rules,
            last_controls: Controls::default(),
            accumulator: 0.,
            team: 0,
        };
        world.generate_cover();
        world.reset(0);
        world
    }
    fn spawn_point(&self, index: usize) -> (Vec3, Quat) {
        let z = if self.team == 0 { 3500. } else { -3500. };
        let forward = if self.team == 0 { -Vec3::Z } else { Vec3::Z };
        let x = if index == 0 {
            0.
        } else {
            (index as f32 - 2.) * 280.
        };
        let position = Vec3::new(x, 0., z) + forward * index as f32 * 500.;
        let y = self.terrain.ground_at(position.x, position.z) + 400. + index as f32 * 40.;
        (
            Vec3::new(position.x, y, position.z),
            if self.team == 0 {
                Quat::IDENTITY
            } else {
                Quat::from_rotation_y(std::f32::consts::PI)
            },
        )
    }
    pub fn reset(&mut self, team: usize) {
        self.team = team;
        self.actors.clear();
        for i in 0..=self.rules.bot_count {
            let (p, q) = self.spawn_point(i);
            self.actors.push(Aircraft::new(p, q, 1, self.rules));
        }
        self.projectiles.clear();
        self.explosions.clear();
        self.kills = 0;
        self.deaths = 0;
        self.shots = 0;
        self.hits = 0;
        self.elapsed = 0.;
        self.finished = false;
        self.accumulator = 0.;
    }
    pub fn respawn_player(&mut self, team: usize) {
        self.team = team;
        let (p, q) = self.spawn_point(0);
        let life = self.actors[0].life.wrapping_add(1);
        self.actors[0] = Aircraft::new(p, q, life, self.rules);
        self.snap_render_state();
    }
    pub fn swap_weapon(&mut self) {
        let player = &mut self.actors[0];
        player.weapon = if player.weapon == Weapon::Guns {
            Weapon::Missiles
        } else {
            Weapon::Guns
        };
        // The shared cooldown is retained, preventing swap exploits.
    }

    pub fn request_respawn(&mut self) {
        if self.actors[0].alive() {
            self.destroy(0, None);
        }
    }
    pub fn advance(&mut self, seconds: f64, input: &FlightInput, scheme: usize, aim: Vec3) {
        self.accumulator += seconds.clamp(0., 0.1);
        while self.accumulator + 1e-10 >= self.rules.fixed_dt() as f64 {
            self.step_with_controls(&[Some(input.controls(scheme, self.actors[0].body, aim))]);
            self.accumulator -= self.rules.fixed_dt() as f64;
            if !self.actors[0].alive() || self.finished {
                self.accumulator = 0.;
                break;
            }
        }
    }
    /// Fixed authoritative tick. Supply each human's controls; None selects the local bot.
    /// This entry point needs no window, input device, renderer, or Python runtime.
    pub fn step_with_controls(&mut self, inputs: &[Option<Controls>]) {
        for actor in &mut self.actors {
            actor.previous_body = actor.body;
        }
        self.elapsed += self.rules.fixed_dt();
        self.last_controls = inputs.first().copied().flatten().unwrap_or_default();
        for i in 0..self.actors.len() {
            if !self.actors[i].alive() {
                if inputs.get(i).copied().flatten().is_none() {
                    self.actors[i].respawn -= self.rules.fixed_dt();
                    if self.actors[i].respawn <= 0. {
                        let (p, q) = self.spawn_point(i);
                        let life = self.actors[i].life.wrapping_add(1);
                        self.actors[i] = Aircraft::new(p, q, life, self.rules);
                    }
                }
                continue;
            }
            let controls = if let Some(controls) = inputs.get(i).copied().flatten() {
                controls
            } else {
                let body = self.actors[i].body;
                let player = self.actors[0].body;
                let chasing = (self.elapsed * 0.12 + i as f32).sin() > 0.;
                let waypoint = if chasing && body.position.distance(player.position) > 220. {
                    player.position + player.velocity * 0.8
                } else {
                    let phase = self.elapsed * 0.10 + i as f32 * 1.7;
                    Vec3::new(
                        phase.sin() * 2300.,
                        self.terrain.max.y + 400. + i as f32 * 60.,
                        phase.cos() * 2300.,
                    )
                };
                let mut c = steer_toward(body, waypoint - body.position);
                // Terrain avoidance uses the actual triangle mesh, not a sampled heightfield.
                if self
                    .terrain
                    .segment_hit(
                        body.position,
                        body.position + body.forward() * 500. - Vec3::Y * 120.,
                    )
                    .is_some()
                    || body.position.y - self.terrain.ground_at(body.position.x, body.position.z)
                        < 120.
                {
                    c.pitch = 0.8;
                }
                c.fire = body
                    .forward()
                    .dot((player.position - body.position).normalize_or_zero())
                    > 0.985
                    && body.position.distance(player.position) < 1800.;
                c
            };
            let previous = self.actors[i].body.position;
            let actor = &mut self.actors[i];
            actor.cooldown = (actor.cooldown - self.rules.fixed_dt()).max(0.);
            actor.invulnerable = (actor.invulnerable - self.rules.fixed_dt()).max(0.);
            actor
                .body
                .step(controls, self.tuning, self.rules.fixed_dt());
            let position = actor.body.position;
            if position.x.abs() > 6400. || position.z.abs() > 6400. {
                actor.outside += self.rules.fixed_dt();
            } else {
                actor.outside = 0.;
            }
            let crashed = self.terrain.segment_hit(previous, position).is_some()
                || self.terrain.overlaps_sphere(position, 5.)
                || self
                    .cover
                    .iter()
                    .any(|c| position.distance_squared(c.center) < (c.radius + 5.).powi(2))
                || position.y < self.terrain.min.y - 100.
                || self.actors[i].outside > 8.;
            if crashed {
                self.destroy(i, None);
            }
        }
        for a in 0..self.actors.len() {
            for b in a + 1..self.actors.len() {
                if self.actors[a].alive()
                    && self.actors[b].alive()
                    && self.actors[a]
                        .body
                        .position
                        .distance_squared(self.actors[b].body.position)
                        < 100.
                {
                    self.destroy(a, None);
                    self.destroy(b, None);
                }
            }
        }
        self.update_radars();
        for i in 0..self.actors.len() {
            if !self.actors[i].alive() {
                continue;
            }
            if let Some(controls) = inputs.get(i).copied().flatten() {
                if controls.fire {
                    self.fire(i);
                }
            } else {
                let player = self.actors[0].body.position;
                let body = self.actors[i].body;
                if self.actors[0].alive()
                    && body
                        .forward()
                        .dot((player - body.position).normalize_or_zero())
                        > 0.99
                    && body.position.distance(player) < 1600.
                {
                    self.actors[i].weapon =
                        if self.actors[i].radar.locked && self.actors[i].missiles > 0 {
                            Weapon::Missiles
                        } else {
                            Weapon::Guns
                        };
                    self.fire(i);
                }
            }
        }
        self.update_projectiles();
        for explosion in &mut self.explosions {
            explosion.age += self.rules.fixed_dt();
        }
        self.explosions.retain(|e| e.age < 0.7);
        self.finished =
            self.elapsed >= self.rules.round_seconds || self.kills >= self.rules.kill_limit;
    }
    fn visible_target(&self, source: usize, target: usize, holding: bool) -> bool {
        let a = &self.actors[source];
        let b = &self.actors[target];
        let difference = b.body.position - a.body.position;
        a.alive()
            && b.alive()
            && source != target
            && (source == 0 || target == 0)
            && difference.length_squared() <= self.rules.radar_range * self.rules.radar_range
            && a.body.forward().dot(difference.normalize_or_zero())
                > if holding {
                    self.rules.radar_hold_degrees
                } else {
                    self.rules.radar_acquire_degrees
                }
                .to_radians()
                .cos()
            && self.line_of_sight(a.body.position, b.body.position)
    }
    pub fn line_of_sight(&self, a: Vec3, b: Vec3) -> bool {
        self.terrain.segment_hit(a, b).is_none()
            && !self
                .cover
                .iter()
                .any(|c| segment_sphere(a, b, c.center, c.radius).is_some())
    }
    fn update_radars(&mut self) {
        for source in 0..self.actors.len() {
            let old = self.actors[source].radar.target;
            let target = old
                .filter(|key| {
                    self.actors
                        .get(key.actor)
                        .is_some_and(|a| a.life == key.life)
                        && self.visible_target(source, key.actor, true)
                })
                .or_else(|| {
                    (0..self.actors.len())
                        .filter(|&target| self.visible_target(source, target, false))
                        .max_by(|&a, &b| {
                            let body = self.actors[source].body;
                            body.forward()
                                .dot(
                                    (self.actors[a].body.position - body.position)
                                        .normalize_or_zero(),
                                )
                                .total_cmp(
                                    &body.forward().dot(
                                        (self.actors[b].body.position - body.position)
                                            .normalize_or_zero(),
                                    ),
                                )
                        })
                        .map(|actor| Target {
                            actor,
                            life: self.actors[actor].life,
                        })
                });
            self.actors[source].radar.update_with_duration(
                target,
                self.rules.fixed_dt(),
                self.rules.radar_acquire_seconds,
            );
        }
    }
    /// Both weapons are fireable without radar lock. A lock only designates missile guidance.
    pub fn fire(&mut self, owner: usize) -> bool {
        if self.projectiles.len() >= self.rules.max_projectiles {
            return false;
        }
        let actor = &mut self.actors[owner];
        if !actor.alive() || actor.cooldown > 0. {
            return false;
        }
        let missile = actor.weapon == Weapon::Missiles;
        if missile {
            if actor.missiles == 0 {
                return false;
            }
            actor.missiles -= 1;
            actor.cooldown = self.rules.missile_cooldown;
        } else {
            if actor.gun_ammo == 0 {
                return false;
            }
            actor.gun_ammo -= 1;
            actor.cooldown = if owner == 0 {
                1. / self.rules.gun_hz
            } else {
                1. / self.rules.bot_gun_hz
            };
        }
        let target = if missile && actor.radar.locked {
            actor.radar.target
        } else {
            None
        };
        let body = actor.body;
        self.projectiles.push(Projectile {
            position: body.position + body.forward() * 12. - body.up() * 0.5,
            velocity: body.velocity + body.forward() * if missile { 240. } else { 850. },
            previous_position: body.position + body.forward() * 12. - body.up() * 0.5,
            previous_velocity: body.velocity + body.forward() * if missile { 240. } else { 850. },
            owner,
            target,
            missile,
            remaining: if missile { 8. } else { 2.5 },
        });
        if owner == 0 {
            self.shots += 1;
        }
        true
    }
    fn update_projectiles(&mut self) {
        let mut index = 0;
        while index < self.projectiles.len() {
            let projectile = &mut self.projectiles[index];
            projectile.previous_position = projectile.position;
            projectile.previous_velocity = projectile.velocity;
            if let Some(target) = projectile.target {
                if let Some(actor) = self
                    .actors
                    .get(target.actor)
                    .filter(|a| a.alive() && a.life == target.life)
                {
                    let intercept = actor.body.position
                        + actor.body.velocity
                            * (projectile.position.distance(actor.body.position) / 650.).min(1.5)
                            * 0.65;
                    let current = projectile.velocity.normalize_or_zero();
                    let desired = (intercept - projectile.position)
                        .try_normalize()
                        .unwrap_or(current);
                    let angle = current.angle_between(desired);
                    let turn = (1.8 * self.rules.fixed_dt() / angle.max(0.0001)).min(1.);
                    let direction = (Quat::IDENTITY
                        .slerp(Quat::from_rotation_arc(current, desired), turn)
                        * current)
                        .normalize();
                    let speed =
                        (projectile.velocity.length() + 70. * self.rules.fixed_dt()).min(650.);
                    projectile.velocity = direction * speed;
                } else {
                    projectile.target = None;
                }
            } else if projectile.missile {
                projectile.velocity +=
                    projectile.velocity.normalize_or_zero() * 35. * self.rules.fixed_dt();
            }
            let start = projectile.position;
            let end = start + projectile.velocity * self.rules.fixed_dt();
            let owner = projectile.owner;
            let missile = projectile.missile;
            let mut nearest = self.terrain.segment_hit(start, end).map(|t| (t, None));
            for cover in &self.cover {
                if let Some(t) = segment_sphere(start, end, cover.center, cover.radius) {
                    if nearest.is_none_or(|hit| t < hit.0) {
                        nearest = Some((t, None));
                    }
                }
            }
            for (actor, aircraft) in self.actors.iter().enumerate() {
                if actor == owner || !aircraft.alive() || aircraft.invulnerable > 0. {
                    continue;
                }
                if let Some(t) = segment_sphere(
                    start,
                    end,
                    aircraft.body.position,
                    if missile { 12. } else { 7. },
                ) {
                    if nearest.is_none_or(|hit| t < hit.0) {
                        nearest = Some((t, Some(actor)));
                    }
                }
            }
            self.projectiles[index].remaining -= self.rules.fixed_dt();
            self.projectiles[index].position = end;
            if let Some((fraction, target)) = nearest {
                let point = start.lerp(end, fraction);
                if let Some(target) = target {
                    self.actors[target].health -= if missile {
                        self.rules.missile_damage
                    } else {
                        self.rules.gun_damage
                    };
                    if owner == 0 {
                        self.hits += 1;
                    }
                    if self.actors[target].health <= 0. {
                        self.destroy(target, Some(owner));
                    }
                }
                self.blast(point, if missile { 14. } else { 3. });
                self.projectiles.swap_remove(index);
            } else if self.projectiles[index].remaining <= 0. {
                self.projectiles.swap_remove(index);
            } else {
                index += 1;
            }
        }
    }
    fn destroy(&mut self, actor: usize, killer: Option<usize>) {
        // Callers can arrive here after health has already crossed zero.
        if self.actors[actor].respawn > 0. {
            return;
        }
        self.actors[actor].health = 0.;
        self.actors[actor].radar = Radar::default();
        self.actors[actor].respawn = self.rules.respawn_seconds;
        if actor == 0 {
            self.deaths += 1;
        } else if killer == Some(0) {
            self.kills += 1;
        }
        self.blast(self.actors[actor].body.position, 24.);
    }
    fn blast(&mut self, position: Vec3, radius: f32) {
        if self.explosions.len() < 48 {
            self.explosions.push(Explosion {
                position,
                age: 0.,
                radius,
            });
        }
    }
    /// Flags are per receiver: outgoing lock, incoming radar lock, incoming guided missile.
    pub fn audio_flags(&self, receiver: usize) -> u8 {
        let Some(actor) = self.actors.get(receiver).filter(|a| a.alive()) else {
            return 0;
        };
        let key = Target {
            actor: receiver,
            life: actor.life,
        };
        u8::from(actor.radar.locked)
            | (u8::from(
                self.actors
                    .iter()
                    .any(|a| a.radar.locked && a.radar.target == Some(key)),
            ) << 1)
            | (u8::from(
                self.projectiles
                    .iter()
                    .any(|p| p.missile && p.target == Some(key)),
            ) << 2)
    }
    fn generate_cover(&mut self) {
        let mut seed = self.rules.cover_seed;
        let mut random = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1_u32 << 24) as f32
        };
        for i in 0..self.rules.cover_count {
            let x = (random() - 0.5) * 11000.;
            let z = (random() - 0.5) * 11000.;
            let ground = self.terrain.ground_at(x, z);
            let tree = i % 2 == 0;
            let size = if tree {
                8. + random() * 10.
            } else {
                4. + random() * 6.
            };
            let center = Vec3::new(x, ground + size * if tree { 0.5 } else { 0.35 }, z);
            let rotation = Quat::from_rotation_y(random() * std::f32::consts::TAU)
                * if tree {
                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)
                } else {
                    Quat::from_rotation_z(0.7)
                };
            let scale = if tree {
                Vec3::new(1.8, 1.8, size / 3.)
            } else {
                Vec3::new(size, size * 0.6, size / 3.)
            };
            self.cover.push(Cover {
                transform: Mat4::from_scale_rotation_translation(scale, rotation, center),
                center,
                radius: size * 0.45,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        flight::FIXED_DT,
        rendering::{Camera, MeshData},
    };
    use glam::Vec2;
    fn world() -> World {
        World::new(Terrain::new(&MeshData::default(), Mat4::IDENTITY))
    }

    #[test]
    fn configured_tick_and_external_aircraft_controls_work_without_renderer() {
        let mut config = GameConfig::default();
        config.rules.physics_hz = 60;
        config.rules.bot_count = 1;
        config.rules.cover_count = 0;
        config.rules.gun_ammo = 30;
        config.rules.missiles = 2;
        config.validate().unwrap();
        let mut world =
            World::configured(Terrain::new(&MeshData::default(), Mat4::IDENTITY), &config);
        assert_eq!(world.actors.len(), 2);
        assert!(world.cover.is_empty());
        assert_eq!(world.actors[1].gun_ammo, 30);
        assert_eq!(world.actors[1].missiles, 2);
        let first = world.actors[0].body.position;
        world.step_with_controls(&[
            Some(Controls::default()),
            Some(Controls {
                fire: true,
                ..Default::default()
            }),
        ]);
        assert!((world.elapsed - 1. / 60.).abs() < 1e-6);
        assert_ne!(world.actors[0].body.position, first);
        assert_eq!(world.actors[0].gun_ammo, 30);
        assert_eq!(world.actors[1].gun_ammo, 29);
    }

    #[test]
    fn half_tick_blends_visuals_without_changing_physics() {
        let mut w = world();
        w.cover.clear();
        w.actors.truncate(1);
        let input = FlightInput::default();
        let dt = FIXED_DT as f64;
        let previous = w.actors[0].body.position;
        w.advance(dt, &input, 0, -Vec3::Z);
        let authoritative = w.actors[0].body.position;
        assert_eq!(w.render_body(0).position, previous);
        w.advance(dt * 0.5, &input, 0, -Vec3::Z);
        assert!((w.render_alpha() - 0.5).abs() < 1e-5);
        assert!(
            w.render_body(0)
                .position
                .distance(previous.lerp(authoritative, 0.5))
                < 1e-4
        );
        assert_eq!(w.actors[0].body.position, authoritative);
    }

    #[test]
    fn render_frames_between_ticks_keep_advancing_smoothly() {
        let mut w = world();
        w.cover.clear();
        w.actors.truncate(1);
        let input = FlightInput::default();
        let dt = FIXED_DT as f64;
        w.advance(dt, &input, 0, -Vec3::Z);
        let mut last = w.render_body(0).position;
        let mut distances = Vec::new();
        for _ in 0..24 {
            w.advance(dt * 0.5, &input, 0, -Vec3::Z);
            let current = w.render_body(0).position;
            distances.push(last.distance(current));
            last = current;
        }
        let min = distances.iter().copied().fold(f32::INFINITY, f32::min);
        let max = distances.iter().copied().fold(0., f32::max);
        assert!(min > 0.5 && max / min < 1.02, "min={min} max={max}");
    }

    #[test]
    fn respawning_snaps_poses_instead_of_sweeping_across_the_map() {
        let mut w = world();
        let old = w.actors[0].body.position;
        w.respawn_player(1);
        assert!(w.actors[0].body.position.distance(old) > 6000.);
        assert_eq!(w.render_body(0).position, w.actors[0].body.position);
        w.actors[1].health = 0.;
        w.actors[1].respawn = 0.;
        w.actors[1].body.position = Vec3::splat(99999.);
        w.advance(FIXED_DT as f64, &FlightInput::default(), 0, Vec3::Z);
        assert!(w.actors[1].alive());
        assert_eq!(w.render_body(1).position, w.actors[1].body.position);
    }
    #[test]
    fn both_weapons_are_equipped_and_unlocked_missiles_are_fireable() {
        let mut w = world();
        assert_eq!(w.actors[0].weapon, Weapon::Guns);
        assert!(w.fire(0));
        assert!(!w.fire(0));
        w.swap_weapon();
        assert!(!w.fire(0)); // swapping does not bypass cooldown
        w.actors[0].cooldown = 0.;
        assert!(w.fire(0));
        assert!(w.projectiles.last().unwrap().missile);
        assert_eq!(w.projectiles.last().unwrap().target, None);
        assert_eq!(w.actors[0].missiles, 7);
        w.respawn_player(0);
        assert_eq!(w.actors[0].weapon, Weapon::Guns);
    }
    #[test]
    fn radar_acquires_then_breaks_on_angle_range_or_occlusion() {
        let mut w = world();
        w.cover.clear();
        w.actors.truncate(2);
        w.actors[0].body = RigidBody::new(Vec3::Y * 100., Quat::IDENTITY);
        w.actors[1].body = RigidBody::new(Vec3::new(0., 100., -500.), Quat::IDENTITY);
        for _ in 0..160 {
            w.update_radars();
        }
        assert!(w.actors[0].radar.locked);
        assert_eq!(w.audio_flags(0) & 1, 1);
        assert_eq!(w.audio_flags(1) & 2, 2);
        w.actors[1].body.position.z = 500.;
        w.update_radars();
        assert!(!w.actors[0].radar.locked);
        w.actors[1].body.position.z = -3000.;
        w.update_radars();
        assert!(w.actors[0].radar.target.is_none());
        w.actors[1].body.position.z = -500.;
        w.cover.push(Cover {
            transform: Mat4::IDENTITY,
            center: Vec3::new(0., 100., -250.),
            radius: 20.,
        });
        for _ in 0..160 {
            w.update_radars();
        }
        assert!(!w.actors[0].radar.locked);
    }
    #[test]
    fn fired_missile_retains_target_after_launching_radar_loses_lock() {
        let mut w = world();
        w.cover.clear();
        w.actors.truncate(2);
        let target = Target {
            actor: 1,
            life: w.actors[1].life,
        };
        w.actors[0].radar = Radar {
            target: Some(target),
            progress: 1.,
            locked: true,
        };
        w.actors[0].weapon = Weapon::Missiles;
        assert!(w.fire(0));
        w.actors[0].radar = Radar::default();
        assert_eq!(w.projectiles[0].target, Some(target));
        w.update_projectiles();
        assert_eq!(w.projectiles[0].target, Some(target));
        w.actors[1].life += 1;
        w.update_projectiles();
        assert_eq!(w.projectiles[0].target, None);
    }

    #[test]
    fn swept_gun_hits_damage_destroy_and_respawn_with_both_weapons() {
        let mut w = world();
        w.cover.clear();
        w.actors.truncate(2);
        w.actors[0].body = RigidBody::new(Vec3::new(0., 100., 0.), Quat::IDENTITY);
        w.actors[1].body = RigidBody::new(Vec3::new(0., 100., -100.), Quat::IDENTITY);
        w.actors[1].invulnerable = 0.;
        for _ in 0..6 {
            w.actors[0].cooldown = 0.;
            assert!(w.fire(0));
            for _ in 0..20 {
                w.update_projectiles();
            }
        }
        assert!(!w.actors[1].alive());
        assert_eq!(w.kills, 1);
        assert_eq!(w.hits, 6);
        w.actors[0].weapon = Weapon::Missiles;
        w.actors[0].gun_ammo = 0;
        w.actors[0].missiles = 0;
        w.respawn_player(0);
        assert_eq!(w.actors[0].weapon, Weapon::Guns);
        assert_eq!((w.actors[0].gun_ammo, w.actors[0].missiles), (800, 8));
    }

    #[test]
    fn pole_camera_and_all_control_modes_survive_sustained_maneuvers() {
        for scheme in 0..4 {
            let mut body = RigidBody::new(Vec3::Y * 2000., Quat::IDENTITY);
            let mut input = FlightInput::default();
            input.set(crate::flight::FlightKey::Down, true);
            input.set(crate::flight::FlightKey::E, true);
            input.mouse = Vec2::new(0.3, 0.2);
            let mut camera = Camera::default();
            for _ in 0..2400 {
                let controls = input.controls(scheme, body, Vec3::new(0.1, 0.3, -1.).normalize());
                body.step(controls, FlightTuning::default(), FIXED_DT);
                chase_camera(&mut camera, body, FIXED_DT, true);
                assert!(body.position.is_finite() && body.orientation.is_finite());
                assert!(camera.view_projection(16. / 9.).unwrap().is_finite());
            }
        }
    }
    #[test]
    fn fixed_step_results_do_not_depend_on_render_frequency() {
        let mut a = world();
        let mut b = world();
        a.cover.clear();
        b.cover.clear();
        let input = FlightInput::default();
        for _ in 0..60 {
            a.advance(1. / 60., &input, 0, -Vec3::Z);
        }
        for _ in 0..240 {
            b.advance(1. / 240., &input, 0, -Vec3::Z);
        }
        assert!(
            a.actors[0]
                .body
                .position
                .distance(b.actors[0].body.position)
                < 0.01
        );
    }
}
