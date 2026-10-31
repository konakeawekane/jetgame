//! GPU assets, camera, and HUD projection, separate from simulation state.

use super::*;
use crate::{
    config::GameConfig,
    rendering::{
        Camera, MeshData, MeshHandle, RenderObject, Renderer, Scene,
        mesh::{Bounds, Vertex},
    },
};
use glam::Vec2;
use std::path::Path;

impl World {
    pub fn hud(
        &self,
        camera: Camera,
        viewport: [u32; 2],
        input: &FlightInput,
        scheme: usize,
    ) -> crate::ui::HudData {
        let player = &self.actors[self.local_actor];
        let player_pose = self.render_body(self.local_actor);
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
        let flags = self.audio_flags(self.local_actor);
        let targets = self
            .actors
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != self.local_actor)
            .filter(|(i, _)| self.enemies(self.local_actor, *i))
            .filter(|(_, a)| a.alive())
            .filter_map(|(i, a)| {
                let pose = self.render_body(i);
                project(pose.position).map(|position| crate::ui::TargetMarker {
                    position,
                    number: i,
                    distance: player_pose.position.distance(pose.position) as u32,
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
            seconds: (self.rules.round_seconds - self.elapsed).max(0.) as u32,
            outside: player.outside > 0.,
            aim: if scheme >= 2 {
                Some([
                    (640. + input.mouse.x * if scheme == 2 { 180. } else { 520. }) as i32,
                    (360. + input.mouse.y * if scheme == 2 { 180. } else { 280. }) as i32,
                ])
            } else {
                None
            },
            sight: project(player_pose.position + player_pose.forward() * 1000.),
            targets,
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
        let config = GameConfig {
            assets: crate::config::AssetConfig {
                root: asset_root.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        Self::load_configured_on(device, assets, jet_path, &config)
    }
    pub fn load_configured_on(
        device: &wgpu::Device,
        assets: &mut crate::rendering::mesh::MeshAssets,
        jet_path: &Path,
        config: &GameConfig,
    ) -> Result<(Self, World), Box<dyn std::error::Error>> {
        let asset_root = &config.assets.root;
        let jet = assets.load(device, jet_path)?;
        let ground = assets.load(device, asset_root.join(&config.assets.terrain))?;
        let rocket = assets.load(device, asset_root.join(&config.assets.missile))?;
        let bullet = assets.load(device, asset_root.join(&config.assets.bullet))?;
        let bounds = assets.get(ground).unwrap().bounds;
        let scale = config.assets.terrain_width
            / (bounds.max.x - bounds.min.x).max(bounds.max.z - bounds.min.z);
        let ground_model = Mat4::from_scale(Vec3::new(
            scale,
            scale * config.assets.terrain_height_scale,
            scale,
        )) * Mat4::from_translation(-Vec3::new(
            bounds.center().x,
            bounds.min.y,
            bounds.center().z,
        ));
        let terrain_data =
            MeshData::from_glb(&std::fs::read(asset_root.join(&config.assets.terrain))?)?;
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
            World::configured(terrain, config),
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
        for (index, actor) in world.actors.iter().enumerate() {
            if actor.alive() {
                let pose = world.render_body(index);
                scene.objects.push(RenderObject {
                    mesh: self.jet,
                    transform: Mat4::from_rotation_translation(pose.orientation, pose.position)
                        * self.jet_model,
                });
            }
        }
        for missile in [false, true] {
            for p in world.projectiles.iter().filter(|p| p.missile == missile) {
                let alpha = world.render_alpha();
                let position = p.previous_position.lerp(p.position, alpha);
                let velocity = p.previous_velocity.lerp(p.velocity, alpha);
                let rotation = Quat::from_rotation_arc(-Vec3::Z, velocity.normalize());
                scene.objects.push(RenderObject {
                    mesh: if missile { self.rocket } else { self.bullet },
                    transform: Mat4::from_rotation_translation(rotation, position)
                        * if missile {
                            self.rocket_model
                        } else {
                            self.bullet_model
                        },
                });
            }
        }
        for e in &world.explosions {
            let age = (e.age - world.rules.fixed_dt()
                + world.render_alpha() * world.rules.fixed_dt())
            .max(0.);
            let size = (age / 0.7).sqrt() * e.radius + 0.5;
            scene.objects.push(RenderObject {
                mesh: self.effect,
                transform: Mat4::from_scale_rotation_translation(
                    Vec3::splat(size),
                    Quat::from_rotation_y(age * 4.),
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
