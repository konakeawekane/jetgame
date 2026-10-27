//! Offline flight/combat simulation. GPU meshes are shared; physics uses small local types.

use crate::{
    flight::{Controls, FIXED_DT, FlightInput, FlightTuning, RigidBody, steer_toward},
    rendering::{
        Camera, MeshData, MeshHandle, RenderObject, Renderer, Scene,
        mesh::{Bounds, Vertex},
    },
    terrain::{Terrain, segment_sphere},
};
use glam::{Mat4, Quat, Vec2, Vec3};
use std::path::Path;

pub const MAX_PROJECTILES: usize = 256;
const RADAR_RANGE: f32 = 2800.;
const ACQUIRE_TIME: f32 = 1.25;

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
        if target != self.target {
            self.target = target;
            self.progress = 0.;
            self.locked = false;
        }
        if target.is_some() {
            self.progress = (self.progress + dt / ACQUIRE_TIME).min(1.);
            self.locked = self.progress >= 1.;
        } else {
            self.progress = 0.;
            self.locked = false;
        }
    }
}

pub struct Aircraft {
    pub body: RigidBody,
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
    fn new(position: Vec3, orientation: Quat, life: u32) -> Self {
        Self {
            body: RigidBody::new(position, orientation),
            health: 100.,
            life,
            radar: Radar::default(),
            weapon: Weapon::Guns,
            gun_ammo: 800,
            missiles: 8,
            cooldown: 0.,
            invulnerable: 2.,
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
    pub last_controls: Controls,
    accumulator: f64,
    team: usize,
}

impl World {
    pub fn hud(
        &self,
        camera: Camera,
        viewport: [u32; 2],
        input: &FlightInput,
        scheme: usize,
    ) -> crate::ui::HudData {
        let player = &self.actors[0];
        let project = |point: Vec3| -> Option<[i32; 2]> {
            let projection = camera
                .view_projection(viewport[0].max(1) as f32 / viewport[1].max(1) as f32)
                .ok()?;
            let clip = projection * point.extend(1.);
            if clip.w <= 0. {
                return None;
            }
            let ndc = clip.truncate() / clip.w;
            if ndc.z < 0. || ndc.z > 1. {
                return None;
            }
            let pixel = [
                (ndc.x * 0.5 + 0.5) * viewport[0] as f32,
                (0.5 - ndc.y * 0.5) * viewport[1] as f32,
            ];
            crate::ui::to_design(pixel, viewport)
                .map(|p| [p[0].round() as i32, p[1].round() as i32])
        };
        let flags = self.audio_flags(0);
        let targets = self
            .actors
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(_, a)| a.alive())
            .filter_map(|(i, a)| {
                project(a.body.position).map(|position| crate::ui::TargetMarker {
                    position,
                    number: i,
                    distance: player.body.position.distance(a.body.position) as u32,
                    health: a.health.max(0.) as u8,
                    locked: player.radar.locked
                        && player.radar.target
                            == Some(Target {
                                actor: i,
                                life: a.life,
                            }),
                })
            })
            .collect();
        crate::ui::HudData {
            speed: (player.body.velocity.length() * 3.6) as u32,
            altitude: player.body.position.y.max(0.) as u32,
            health: player.health.max(0.) as u32,
            weapon: if player.weapon == Weapon::Guns {
                "GUNS"
            } else {
                "MISSILES"
            },
            ammo: if player.weapon == Weapon::Guns {
                player.gun_ammo
            } else {
                player.missiles
            },
            missiles: player.missiles,
            throttle: (self.last_controls.throttle * 100.) as u32,
            radar_progress: (player.radar.progress * 100.) as u32,
            radar_locked: player.radar.locked,
            incoming_lock: flags & 2 != 0,
            incoming_missile: flags & 4 != 0,
            kills: self.kills,
            deaths: self.deaths,
            shots: self.shots,
            hits: self.hits,
            seconds: (180. - self.elapsed).max(0.) as u32,
            outside: player.outside > 0.,
            aim: if scheme >= 2 {
                Some([
                    (640. + input.mouse.x * if scheme == 2 { 180. } else { 520. }) as i32,
                    (360. + input.mouse.y * if scheme == 2 { 180. } else { 280. }) as i32,
                ])
            } else {
                None
            },
            sight: project(player.body.position + player.body.forward() * 1000.),
            targets,
        }
    }
    pub fn new(terrain: Terrain) -> Self {
        let mut world = Self {
            terrain,
            actors: Vec::with_capacity(4),
            projectiles: Vec::with_capacity(MAX_PROJECTILES),
            explosions: Vec::with_capacity(32),
            cover: Vec::with_capacity(72),
            kills: 0,
            deaths: 0,
            shots: 0,
            hits: 0,
            elapsed: 0.,
            finished: false,
            tuning: FlightTuning::default(),
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
        for i in 0..4 {
            let (p, q) = self.spawn_point(i);
            self.actors.push(Aircraft::new(p, q, 1));
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
        self.actors[0] = Aircraft::new(p, q, life);
        self.accumulator = 0.;
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
        while self.accumulator + 1e-10 >= FIXED_DT as f64 {
            self.step(input.controls(scheme, self.actors[0].body, aim));
            self.accumulator -= FIXED_DT as f64;
            if !self.actors[0].alive() || self.finished {
                self.accumulator = 0.;
                break;
            }
        }
    }
    fn step(&mut self, player_input: Controls) {
        self.elapsed += FIXED_DT;
        self.last_controls = player_input;
        for i in 0..self.actors.len() {
            if !self.actors[i].alive() {
                if i > 0 {
                    self.actors[i].respawn -= FIXED_DT;
                    if self.actors[i].respawn <= 0. {
                        let (p, q) = self.spawn_point(i);
                        let life = self.actors[i].life.wrapping_add(1);
                        self.actors[i] = Aircraft::new(p, q, life);
                    }
                }
                continue;
            }
            let controls = if i == 0 {
                player_input
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
            actor.cooldown = (actor.cooldown - FIXED_DT).max(0.);
            actor.invulnerable = (actor.invulnerable - FIXED_DT).max(0.);
            actor.body.step(controls, self.tuning, FIXED_DT);
            let position = actor.body.position;
            if position.x.abs() > 6400. || position.z.abs() > 6400. {
                actor.outside += FIXED_DT;
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
            if i == 0 {
                if player_input.fire {
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
            explosion.age += FIXED_DT;
        }
        self.explosions.retain(|e| e.age < 0.7);
        self.finished = self.elapsed >= 180. || self.kills >= 5;
    }
    fn visible_target(&self, source: usize, target: usize, holding: bool) -> bool {
        let a = &self.actors[source];
        let b = &self.actors[target];
        let difference = b.body.position - a.body.position;
        a.alive()
            && b.alive()
            && source != target
            && (source == 0 || target == 0)
            && difference.length_squared() <= RADAR_RANGE * RADAR_RANGE
            && a.body.forward().dot(difference.normalize_or_zero())
                > if holding { 0.70710677 } else { 0.8660254 }
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
            self.actors[source].radar.update(target, FIXED_DT);
        }
    }
    /// Both weapons are fireable without radar lock. A lock only designates missile guidance.
    pub fn fire(&mut self, owner: usize) -> bool {
        if self.projectiles.len() >= MAX_PROJECTILES {
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
            actor.cooldown = 0.9;
        } else {
            if actor.gun_ammo == 0 {
                return false;
            }
            actor.gun_ammo -= 1;
            actor.cooldown = if owner == 0 { 1. / 12. } else { 0.4 };
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
                    let turn = (1.8 * FIXED_DT / angle.max(0.0001)).min(1.);
                    let direction = (Quat::IDENTITY
                        .slerp(Quat::from_rotation_arc(current, desired), turn)
                        * current)
                        .normalize();
                    let speed = (projectile.velocity.length() + 70. * FIXED_DT).min(650.);
                    projectile.velocity = direction * speed;
                } else {
                    projectile.target = None;
                }
            } else if projectile.missile {
                projectile.velocity += projectile.velocity.normalize_or_zero() * 35. * FIXED_DT;
            }
            let start = projectile.position;
            let end = start + projectile.velocity * FIXED_DT;
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
            self.projectiles[index].remaining -= FIXED_DT;
            self.projectiles[index].position = end;
            if let Some((fraction, target)) = nearest {
                let point = start.lerp(end, fraction);
                if let Some(target) = target {
                    self.actors[target].health -= if missile { 75. } else { 18. };
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
        self.actors[actor].respawn = 5.;
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
        let mut seed = 12345_u32;
        let mut random = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1_u32 << 24) as f32
        };
        for i in 0..72 {
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

pub struct GameAssets {
    jet: MeshHandle,
    ground: MeshHandle,
    rocket: MeshHandle,
    bullet: MeshHandle,
    effect: MeshHandle,
    jet_model: Mat4,
    rocket_model: Mat4,
    bullet_model: Mat4,
    ground_model: Mat4,
}

impl GameAssets {
    pub fn load(
        renderer: &mut Renderer,
        jet_path: &Path,
    ) -> Result<(Self, World), Box<dyn std::error::Error>> {
        let (device, assets) = renderer.mesh_resources();
        Self::load_on(device, assets, jet_path, Path::new("assets"))
    }
    pub fn load_on(
        device: &wgpu::Device,
        assets: &mut crate::rendering::mesh::MeshAssets,
        jet_path: &Path,
        asset_root: &Path,
    ) -> Result<(Self, World), Box<dyn std::error::Error>> {
        let jet = assets.load(device, jet_path)?;
        let ground = assets.load(device, asset_root.join("world_01.glb"))?;
        let rocket = assets.load(device, asset_root.join("rocket.glb"))?;
        let bullet = assets.load(device, asset_root.join("bullet.glb"))?;
        let bounds = assets.get(ground).unwrap().bounds;
        let scale = 12000. / (bounds.max.x - bounds.min.x).max(bounds.max.z - bounds.min.z);
        let ground_model = Mat4::from_scale(Vec3::new(scale, scale * 0.22, scale))
            * Mat4::from_translation(-Vec3::new(
                bounds.center().x,
                bounds.min.y,
                bounds.center().z,
            ));
        let terrain_data = MeshData::from_glb(&std::fs::read(asset_root.join("world_01.glb"))?)?;
        let terrain = Terrain::new(&terrain_data, ground_model);
        let jet_model = normalized_model(assets.get(jet).unwrap().bounds, 15.7, Quat::IDENTITY);
        let rocket_model = normalized_model(assets.get(rocket).unwrap().bounds, 3., Quat::IDENTITY);
        let bullet_model = normalized_model(
            assets.get(bullet).unwrap().bounds,
            4.,
            Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2),
        );
        let effect = assets.upload(device, "Explosion octahedron", &octahedron())?;
        Ok((
            Self {
                jet,
                ground,
                rocket,
                bullet,
                effect,
                jet_model,
                rocket_model,
                bullet_model,
                ground_model,
            },
            World::new(terrain),
        ))
    }
    pub fn preview_scene(&self, scene: &mut Scene) {
        scene.objects.clear();
        scene.objects.push(RenderObject {
            mesh: self.jet,
            transform: self.jet_model,
        });
        scene.camera = Camera::default();
        scene.camera.frame_bounds(
            Bounds {
                min: Vec3::new(-8., -3., -8.),
                max: Vec3::new(8., 3., 8.),
            },
            1.8,
        );
        scene.background = [0.025, 0.035, 0.055, 1.];
    }
    pub fn sync_scene(&self, world: &World, scene: &mut Scene) {
        scene.objects.clear();
        scene.background = [0.16, 0.30, 0.47, 1.];
        scene.objects.push(RenderObject {
            mesh: self.ground,
            transform: self.ground_model,
        });
        for cover in &world.cover {
            scene.objects.push(RenderObject {
                mesh: self.rocket,
                transform: cover.transform * self.rocket_model,
            });
        }
        for actor in &world.actors {
            if actor.alive() {
                scene.objects.push(RenderObject {
                    mesh: self.jet,
                    transform: Mat4::from_rotation_translation(
                        actor.body.orientation,
                        actor.body.position,
                    ) * self.jet_model,
                });
            }
        }
        for missile in [false, true] {
            for p in world.projectiles.iter().filter(|p| p.missile == missile) {
                let rotation = Quat::from_rotation_arc(-Vec3::Z, p.velocity.normalize());
                scene.objects.push(RenderObject {
                    mesh: if missile { self.rocket } else { self.bullet },
                    transform: Mat4::from_rotation_translation(rotation, p.position)
                        * if missile {
                            self.rocket_model
                        } else {
                            self.bullet_model
                        },
                });
            }
        }
        for e in &world.explosions {
            let size = (e.age / 0.7).sqrt() * e.radius + 0.5;
            scene.objects.push(RenderObject {
                mesh: self.effect,
                transform: Mat4::from_scale_rotation_translation(
                    Vec3::splat(size),
                    Quat::from_rotation_y(e.age * 4.),
                    e.position,
                ),
            });
        }
    }
}

pub fn chase_camera(camera: &mut Camera, body: RigidBody, dt: f32, snap: bool) {
    let desired = body.position - body.forward() * 48. + Vec3::Y * 14.;
    camera.eye = if snap {
        desired
    } else {
        camera.eye.lerp(desired, 1. - (-dt * 9.).exp())
    };
    camera.target = body.position + body.forward() * 1800.;
    camera.up = body.up().lerp(Vec3::Y, 0.85).normalize();
    camera.vertical_fov = 60_f32.to_radians();
    camera.near = 0.5;
    camera.far = 18000.;
}

pub fn aim_direction(camera: Camera, mouse: Vec2, viewport: [u32; 2]) -> Vec3 {
    let width = viewport[0].max(1) as f32;
    let height = viewport[1].max(1) as f32;
    let scale = (width / 1280.).min(height / 720.);
    let point = Vec2::new(
        width * 0.5 + mouse.x * 520. * scale,
        height * 0.5 + mouse.y * 280. * scale,
    );
    let ndc = Vec2::new(point.x / width * 2. - 1., 1. - point.y / height * 2.);
    let forward = (camera.target - camera.eye).normalize();
    let right = forward.cross(camera.up).normalize();
    let up = right.cross(forward);
    (forward
        + right * ndc.x * (camera.vertical_fov * 0.5).tan() * width / height
        + up * ndc.y * (camera.vertical_fov * 0.5).tan())
    .normalize()
}

fn normalized_model(bounds: Bounds, length: f32, rotation: Quat) -> Mat4 {
    let size = rotation * (bounds.max - bounds.min);
    Mat4::from_scale(Vec3::splat(length / size.z.abs().max(0.01)))
        * Mat4::from_quat(rotation)
        * Mat4::from_translation(-bounds.center())
}

fn octahedron() -> MeshData {
    let p = [Vec3::Y, -Vec3::Y, Vec3::X, -Vec3::X, Vec3::Z, -Vec3::Z];
    let mut mesh = MeshData::default();
    for [a, b, c] in [
        [0, 4, 2],
        [0, 3, 4],
        [0, 5, 3],
        [0, 2, 5],
        [1, 2, 4],
        [1, 4, 3],
        [1, 3, 5],
        [1, 5, 2],
    ] {
        let normal = (p[b] - p[a]).cross(p[c] - p[a]).normalize().to_array();
        let base = mesh.vertices.len() as u32;
        for i in [a, b, c] {
            mesh.vertices.push(Vertex {
                position: p[i].to_array(),
                normal,
            });
        }
        mesh.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    fn world() -> World {
        World::new(Terrain::new(&MeshData::default(), Mat4::IDENTITY))
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
