use crate::{
    audio::RadarAudio,
    flight::{FlightInput, FlightKey},
    game::{GameAssets, aim_direction, chase_camera},
    networking::{NetworkClient, NetworkConfig},
    rendering::{FrameContext, RenderError, Renderer},
    ui::{Screen, SessionEvent, UiController, UiKey},
};
use glam::Vec2;
use std::{
    error::Error,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    dpi::PhysicalSize,
    event::{DeviceEvent, ElementState, Event, Ime, MouseButton, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, WindowBuilder},
};

pub struct RunOptions {
    pub smoke_test: bool,
    pub preview: bool,
    pub mesh: PathBuf,
    pub offline: bool,
    pub network: NetworkConfig,
    pub connect: Option<String>,
}
impl Default for RunOptions {
    fn default() -> Self {
        Self {
            smoke_test: false,
            preview: false,
            mesh: "assets/f-16_fighter_jet.glb".into(),
            offline: false,
            network: NetworkConfig::default(),
            connect: None,
        }
    }
}

pub fn run(options: RunOptions) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new()?;
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("JetGame")
            .with_inner_size(PhysicalSize::new(1280, 720))
            .build(&event_loop)?,
    );
    let mut renderer = pollster::block_on(Renderer::new(Arc::clone(&window)))?;
    let (assets, mut world) = GameAssets::load(&mut renderer, &options.mesh)?;
    assets.preview_scene(&mut renderer.scene);
    let audio = RadarAudio::new();
    let mut input = FlightInput::default();
    let mut ui = UiController::default();
    let mut network = if options.offline || options.preview {
        None
    } else {
        ui.enable_networking();
        Some(NetworkClient::new(options.network))
    };
    if let Some(endpoint) = options.connect {
        ui.connect_direct(endpoint);
    }
    if options.preview {
        ui.start_preview();
        assets.sync_scene(&world, &mut renderer.scene);
        chase_camera(&mut renderer.scene.camera, world.actors[0].body, 0., true);
    }
    let start = Instant::now();
    let mut previous_frame = start;
    let mut previous_simulation = start;
    let mut previous_tick = start;
    let mut next_tick = start;
    let mut previous_screen = ui.screen();
    let mut first_spawn = !options.preview;
    let mut frame_index = 0;
    let mut cursor = [0_f32; 2];
    let mut occluded = false;
    let mut focused = true;
    let mut ime_allowed = false;
    let mut captured = false;
    let mut fatal_error = None;

    event_loop.run(|event, target| {
        target.set_control_flow(ControlFlow::WaitUntil(next_tick));
        match event {
            Event::WindowEvent { window_id, event } if window_id == window.id() => match event {
                WindowEvent::CloseRequested => target.exit(),
                WindowEvent::Resized(size) => {
                    renderer.resize(size);
                    ui.pointer_move(cursor, [size.width, size.height]);
                    window.request_redraw();
                }
                WindowEvent::Occluded(value) => {
                    occluded = value;
                    if !value {
                        window.request_redraw();
                    }
                }
                WindowEvent::Focused(value) => {
                    focused = value;
                    if !value {
                        input.clear();
                        ui.cancel_pointer();
                        if ui.is_gameplay() {
                            ui.key(UiKey::Pause);
                        }
                    }
                }
                WindowEvent::CursorLeft { .. } => {
                    if !captured {
                        ui.cancel_pointer();
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    cursor = [position.x as f32, position.y as f32];
                    if !ui.is_gameplay() {
                        let size = window.inner_size();
                        ui.pointer_move(cursor, [size.width, size.height]);
                    }
                }
                WindowEvent::MouseInput {
                    state,
                    button: MouseButton::Left,
                    ..
                } => {
                    if !ui.is_gameplay() {
                        ui.pointer_button(state == ElementState::Pressed);
                    }
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        if event.state == ElementState::Released {
                            if let Some(key) = flight_key(code) {
                                input.set(key, false);
                            }
                        } else if ui.is_gameplay() {
                            match code {
                                KeyCode::KeyP | KeyCode::Escape if !event.repeat => {
                                    ui.key(UiKey::Pause);
                                }
                                KeyCode::KeyF if !event.repeat => world.swap_weapon(),
                                _ => {
                                    if let Some(key) = flight_key(code) {
                                        input.set(key, true);
                                    }
                                }
                            }
                        } else {
                            let key = menu_key(code);
                            if ui.editing_text() && key.is_none_or(|k| k == UiKey::Pause) {
                                if let Some(text) = &event.text {
                                    ui.text(text);
                                }
                            } else if let Some(key) = key {
                                if !event.repeat
                                    || matches!(
                                        key,
                                        UiKey::Up
                                            | UiKey::Down
                                            | UiKey::Left
                                            | UiKey::Right
                                            | UiKey::Backspace
                                    )
                                {
                                    ui.key(key);
                                }
                            }
                        }
                    }
                }
                WindowEvent::Ime(Ime::Commit(text)) => ui.text(&text),
                WindowEvent::RedrawRequested => {
                    let now = Instant::now();
                    let frame = FrameContext {
                        frame_index,
                        delta_time: now.duration_since(previous_frame),
                        elapsed_time: now.duration_since(start),
                    };
                    previous_frame = now;
                    let mut outcome = None;
                    if ui.is_gameplay() {
                        let size = window.inner_size();
                        let viewport = [size.width, size.height];
                        let ray = aim_direction(renderer.scene.camera, input.mouse, viewport);
                        let aim = (renderer.scene.camera.eye
                            + ray
                                * renderer
                                    .scene
                                    .camera
                                    .eye
                                    .distance(renderer.scene.camera.target)
                            - world.actors[0].body.position)
                            .normalize();
                        world.advance(
                            now.duration_since(previous_simulation).as_secs_f64(),
                            &input,
                            ui.settings.control_scheme,
                            aim,
                        );
                        previous_simulation = now;
                        // Presentation follows the same interpolated poses as the visible aircraft.
                        chase_camera(
                            &mut renderer.scene.camera,
                            world.render_body(0),
                            frame.delta_time.as_secs_f32().min(0.1),
                            false,
                        );
                        assets.sync_scene(&world, &mut renderer.scene);
                        ui.set_hud(world.hud(
                            renderer.scene.camera,
                            viewport,
                            &input,
                            ui.settings.control_scheme,
                        ));
                        audio.update(world.audio_flags(0), ui.settings.volume);
                        if world.finished {
                            outcome = Some(SessionEvent::MatchEnded);
                        } else if !world.actors[0].alive() {
                            outcome = Some(SessionEvent::AircraftDestroyed);
                        }
                    }
                    renderer.set_vsync(ui.settings.vsync);
                    match renderer.render_with_ui(&frame, ui.frame()) {
                        Ok(true) => {
                            frame_index += 1;
                            if options.smoke_test && frame_index >= 3 {
                                println!("Smoke test presented {frame_index} frames with UI");
                                target.exit();
                            }
                        }
                        Ok(false) => {}
                        Err(RenderError::Surface(
                            wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated,
                        )) => renderer.resize(window.inner_size()),
                        Err(RenderError::Surface(wgpu::SurfaceError::Timeout)) => {
                            log::warn!("Surface frame timed out")
                        }
                        Err(error) => {
                            fatal_error = Some(error.to_string());
                            target.exit();
                        }
                    }
                    if let Some(event) = outcome {
                        ui.session_event(event);
                    }
                }
                _ => {}
            },
            Event::DeviceEvent {
                event: DeviceEvent::MouseMotion { delta },
                ..
            } if ui.is_gameplay() && focused && ui.settings.control_scheme >= 2 => {
                let size = window.inner_size();
                input.mouse_motion(
                    Vec2::new(delta.0 as f32, delta.1 as f32),
                    [size.width, size.height],
                    ui.settings.control_scheme,
                );
            }
            Event::AboutToWait => {
                let now = Instant::now();
                if now >= next_tick {
                    let delta = now.duration_since(previous_tick);
                    previous_tick = now;
                    ui.tick(delta);
                    let size = window.inner_size();
                    if !occluded
                        && size.width > 0
                        && size.height > 0
                        && (ui.is_gameplay() || ui.needs_redraw() || options.smoke_test)
                    {
                        window.request_redraw();
                    }
                    next_tick = now
                        + Duration::from_secs_f64(if ui.is_gameplay() {
                            1. / 120.
                        } else {
                            1. / 30.
                        });
                    target.set_control_flow(ControlFlow::WaitUntil(next_tick));
                }
            }
            _ => {}
        }
        if let Some(client) = &mut network {
            for event in client.poll() {
                ui.network_event(event);
            }
            for request in ui.take_network_requests() {
                if let Err(error) = client.submit(request) {
                    ui.network_request_failed(error);
                }
            }
        }
        let screen = ui.screen();
        if screen != previous_screen {
            input.clear();
            audio.update(0, ui.settings.volume);
            match screen {
                Screen::Spawn => {
                    if matches!(previous_screen, Screen::Joining | Screen::Lobby) {
                        world.reset(ui.team());
                        first_spawn = true;
                    } else if previous_screen == Screen::Pause {
                        world.request_respawn();
                    }
                    assets.preview_scene(&mut renderer.scene);
                }
                Screen::Gameplay if previous_screen == Screen::Spawn => {
                    if first_spawn {
                        world.reset(ui.team());
                        first_spawn = false;
                    }
                    world.respawn_player(ui.team());
                    assets.sync_scene(&world, &mut renderer.scene);
                    chase_camera(&mut renderer.scene.camera, world.actors[0].body, 0., true);
                    let size = window.inner_size();
                    ui.set_hud(world.hud(
                        renderer.scene.camera,
                        [size.width, size.height],
                        &input,
                        ui.settings.control_scheme,
                    ));
                }
                _ => {}
            }
            previous_tick = Instant::now();
            previous_frame = previous_tick;
            previous_simulation = previous_tick;
            next_tick = previous_tick;
            previous_screen = screen;
            window.request_redraw();
        }
        let should_capture = ui.is_gameplay() && focused && ui.settings.control_scheme >= 2;
        if should_capture != captured {
            if should_capture {
                if let Err(error) = window
                    .set_cursor_grab(CursorGrabMode::Confined)
                    .or_else(|_| window.set_cursor_grab(CursorGrabMode::Locked))
                {
                    log::warn!("Cursor capture unavailable: {error}");
                }
            } else {
                let _ = window.set_cursor_grab(CursorGrabMode::None);
            }
            window.set_cursor_visible(!should_capture);
            captured = should_capture;
        }
        if ui.needs_redraw() && !ui.is_gameplay() && !occluded {
            let size = window.inner_size();
            if size.width > 0 && size.height > 0 {
                window.request_redraw();
            }
        }
        if ui.editing_text() != ime_allowed {
            ime_allowed = ui.editing_text();
            window.set_ime_allowed(ime_allowed);
        }
        if ui.quit_requested {
            target.exit();
        }
    })?;
    if let Some(error) = fatal_error {
        return Err(error.into());
    }
    if options.smoke_test && frame_index < 3 {
        return Err("Smoke test ended before three frames were presented".into());
    }
    Ok(())
}
fn menu_key(code: KeyCode) -> Option<UiKey> {
    Some(match code {
        KeyCode::ArrowUp => UiKey::Up,
        KeyCode::ArrowDown => UiKey::Down,
        KeyCode::ArrowLeft => UiKey::Left,
        KeyCode::ArrowRight => UiKey::Right,
        KeyCode::Enter | KeyCode::NumpadEnter => UiKey::Enter,
        KeyCode::KeyP => UiKey::Pause,
        KeyCode::Escape => UiKey::Back,
        KeyCode::Backspace => UiKey::Backspace,
        _ => return None,
    })
}
fn flight_key(code: KeyCode) -> Option<FlightKey> {
    Some(match code {
        KeyCode::KeyW => FlightKey::W,
        KeyCode::KeyS => FlightKey::S,
        KeyCode::KeyA => FlightKey::A,
        KeyCode::KeyD => FlightKey::D,
        KeyCode::KeyQ => FlightKey::Q,
        KeyCode::KeyE => FlightKey::E,
        KeyCode::ArrowUp => FlightKey::Up,
        KeyCode::ArrowDown => FlightKey::Down,
        KeyCode::ArrowLeft => FlightKey::Left,
        KeyCode::ArrowRight => FlightKey::Right,
        KeyCode::Space => FlightKey::Fire,
        _ => return None,
    })
}
