//! Explicit visual check: cargo test --test ui_gpu -- --ignored --nocapture

use std::{fs::File, io::BufWriter, path::Path, sync::mpsc, time::Duration};

use jetgame::{
    rendering::{
        RenderObject, Scene, context::DEPTH_FORMAT, forward::ForwardPipeline, mesh::MeshAssets,
        ui_gpu::UiRenderer,
    },
    ui::{Screen, UiController, UiKey},
};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

struct Harness {
    device: wgpu::Device,
    queue: wgpu::Queue,
    overlay: UiRenderer,
    forward: ForwardPipeline,
    assets: MeshAssets,
    scene: Scene,
}

impl Harness {
    fn capture(&mut self, ui: &mut UiController, root: &Path, name: &str) -> Vec<u8> {
        let frame = ui.frame();
        let world_viewport = frame.world_viewport([WIDTH, HEIGHT]);
        self.overlay
            .prepare(&self.device, &self.queue, [WIDTH, HEIGHT], frame);
        assert!(self.overlay.quad_count() < 4096, "keep UI geometry small");
        if frame.show_world {
            self.forward
                .prepare(
                    &self.device,
                    &self.queue,
                    world_viewport.map_or([WIDTH, HEIGHT], |r| [r[2] as u32, r[3] as u32]),
                    &self.assets,
                    &self.scene,
                )
                .unwrap();
        }
        let size = wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        };
        let color = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("UI preview color"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("UI preview world depth"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Preview world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: self.scene.background[0],
                            g: self.scene.background[1],
                            b: self.scene.background[2],
                            a: self.scene.background[3],
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: frame.show_world.then_some(
                    wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    },
                ),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if frame.show_world {
                if let Some([x, y, width, height]) = world_viewport {
                    pass.set_viewport(x, y, width, height, 0., 1.);
                }
                self.forward.draw(&mut pass, &self.assets, &self.scene);
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Preview UI"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.overlay.draw(&mut pass);
        }
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("UI preview readback"),
            size: (WIDTH * HEIGHT * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::ImageCopyBuffer {
                buffer: &readback,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(WIDTH * 4),
                    rows_per_image: Some(HEIGHT),
                },
            },
            size,
        );
        self.queue.submit(Some(encoder.finish()));
        let (sender, receiver) = mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender.send(result).unwrap();
            });
        self.device.poll(wgpu::Maintain::Wait);
        receiver.recv().unwrap().unwrap();
        let pixels = readback.slice(..).get_mapped_range().to_vec();
        readback.unmap();
        assert!(
            pixels
                .chunks_exact(4)
                .filter(|p| p[0] > 150 || p[1] > 150)
                .count()
                > 500,
            "{name}: expected visible UI text / imagery"
        );
        let output = root.join(format!("{name}.png"));
        let mut png = png::Encoder::new(
            BufWriter::new(File::create(&output).unwrap()),
            WIDTH,
            HEIGHT,
        );
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
        println!(
            "{name}: {} quads / one UI draw; {}",
            self.overlay.quad_count(),
            output.display()
        );
        pixels
    }
}

fn keys(ui: &mut UiController, keys: &[UiKey]) {
    for key in keys {
        ui.key(*key);
    }
}

#[test]
#[ignore = "requires a hardware graphics adapter"]
fn every_screen_renders_and_keyboard_route_remains_functional() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: if cfg!(target_os = "windows") {
            wgpu::Backends::DX12
        } else {
            wgpu::Backends::PRIMARY
        },
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .expect("graphics adapter");
    println!("UI GPU adapter: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("UI test device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
        },
        None,
    ))
    .unwrap();
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = root.join("target/ui-previews");
    std::fs::create_dir_all(&output).unwrap();
    let overlay = UiRenderer::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        &root.join("assets"),
    );
    let forward = ForwardPipeline::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
    let mut assets = MeshAssets::default();
    let jet = assets
        .load(&device, root.join("assets/f-16_fighter_jet.glb"))
        .unwrap();
    let mut scene = Scene::default();
    scene.objects.push(RenderObject::new(jet));
    scene.camera.frame_bounds(
        assets.get(jet).unwrap().bounds,
        WIDTH as f32 / HEIGHT as f32,
    );
    let mut harness = Harness {
        device,
        queue,
        overlay,
        forward,
        assets,
        scene,
    };
    let mut ui = UiController::default();
    harness.capture(&mut ui, &output, "01_splash");
    ui.key(UiKey::Enter);
    harness.capture(&mut ui, &output, "02_start");
    keys(&mut ui, &[UiKey::Down, UiKey::Enter]);
    harness.capture(&mut ui, &output, "03_settings");
    keys(&mut ui, &[UiKey::Back, UiKey::Enter]);
    harness.capture(&mut ui, &output, "04_browser");
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
    harness.capture(&mut ui, &output, "05_joining");
    ui.tick(Duration::from_secs(2));
    harness.capture(&mut ui, &output, "06_lobby");
    keys(&mut ui, &[UiKey::Enter, UiKey::Down, UiKey::Enter]);
    harness.capture(&mut ui, &output, "07_spawn");
    keys(&mut ui, &[UiKey::Down, UiKey::Down, UiKey::Enter]);
    let gameplay = harness.capture(&mut ui, &output, "08_gameplay");
    ui.key(UiKey::Pause);
    let pause = harness.capture(&mut ui, &output, "09_pause");
    assert_ne!(gameplay, pause);
    keys(
        &mut ui,
        &[UiKey::Down, UiKey::Down, UiKey::Down, UiKey::Enter],
    );
    harness.capture(&mut ui, &output, "10_report");
    keys(&mut ui, &[UiKey::Enter, UiKey::Back]);
    keys(
        &mut ui,
        &[
            UiKey::Down,
            UiKey::Down,
            UiKey::Down,
            UiKey::Down,
            UiKey::Down,
            UiKey::Enter,
        ],
    );
    assert_eq!(ui.screen(), Screen::Host);
    harness.capture(&mut ui, &output, "11_host");
    keys(&mut ui, &[UiKey::Back]);
    assert_eq!(ui.screen(), Screen::ServerBrowser);
    harness.overlay = UiRenderer::new(
        &harness.device,
        &harness.queue,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        &root.join("target/__missing_ui_art_test"),
    );
    harness.capture(&mut ui, &output, "12_missing_art_fallback");
    harness.overlay = UiRenderer::new(
        &harness.device,
        &harness.queue,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        &root.join("assets"),
    );
    let (game_assets, mut world) = jetgame::game::GameAssets::load_on(
        &harness.device,
        &mut harness.assets,
        &root.join("assets/f-16_fighter_jet.glb"),
        &root.join("assets"),
    )
    .unwrap();
    let input = jetgame::flight::FlightInput::default();
    let mut camera = jetgame::rendering::Camera::default();
    jetgame::game::chase_camera(&mut camera, world.actors[0].body, 0., true);
    for _ in 0..360 {
        let ray = jetgame::game::aim_direction(camera, input.mouse, [WIDTH, HEIGHT]);
        let aim = (camera.eye + ray * camera.eye.distance(camera.target)
            - world.actors[0].body.position)
            .normalize();
        world.advance(jetgame::flight::FIXED_DT as f64, &input, 3, aim);
        jetgame::game::chase_camera(
            &mut camera,
            world.actors[0].body,
            jetgame::flight::FIXED_DT,
            false,
        );
    }
    assert!(
        world.actors[0].alive(),
        "neutral mouse flight should survive startup"
    );
    assert!(world.actors.iter().all(|a| a.body.position.is_finite()));
    assert_eq!(world.cover.len(), 72);
    game_assets.sync_scene(&world, &mut harness.scene);
    harness.scene.camera = camera;
    ui.start_preview();
    ui.set_hud(world.hud(camera, [WIDTH, HEIGHT], &input, 3));
    harness.capture(&mut ui, &output, "13_live_flight");
    world.reset(0);
    world.actors.truncate(2);
    let position = world.actors[0].body.position;
    world.actors[1].body = jetgame::flight::RigidBody::new(
        position - glam::Vec3::Z * 650.,
        glam::Quat::from_rotation_y(std::f32::consts::PI),
    );
    let player = jetgame::game::Target {
        actor: 0,
        life: world.actors[0].life,
    };
    let enemy = jetgame::game::Target {
        actor: 1,
        life: world.actors[1].life,
    };
    world.actors[0].radar.update(Some(enemy), 2.);
    world.actors[1].radar.update(Some(player), 2.);
    world.actors[0].weapon = jetgame::game::Weapon::Missiles;
    world.actors[1].weapon = jetgame::game::Weapon::Missiles;
    assert!(world.fire(0));
    assert!(world.fire(1));
    assert_eq!(world.audio_flags(0) & 7, 7);
    game_assets.sync_scene(&world, &mut harness.scene);
    jetgame::game::chase_camera(&mut harness.scene.camera, world.actors[0].body, 0., true);
    ui.set_hud(world.hud(harness.scene.camera, [WIDTH, HEIGHT], &input, 3));
    harness.capture(&mut ui, &output, "14_radar_and_missiles");
    assert!(pollster::block_on(harness.device.pop_error_scope()).is_none());
}
