# JetGame

Rust engine for a low-poly multiplayer jet dogfighting game, using `wgpu 0.19`
and `winit 0.29`. Rust owns the game/UI; a separate Python process owns networking.
See [networking setup and packaging](networking/README.md) and [source organization](architecture.md).

## Current rendering path

A single forward pipeline draws every rigid mesh using one universal opaque
Blinn-Phong material. It supplies ambient light, directional diffuse light, and
a specular highlight. Mesh position/normal data and object transforms feed the
same shader for jets, terrain, and other standard 3D objects.

The application opens a resizable 1280x720 window with a splash screen and start
menu. Gameplay now flies the supplied F-16 over the terrain with a following
camera and three local opponents. Depth testing and back-face culling are enabled. All meshes
receive the same gray material; embedded material assignments, textures, vertex
colors, and HDR skies are deliberately unused for 3D meshes at this stage. The UI
uses a separate unlit overlay with its own small image/font atlas.

Windows uses Direct3D 12 after default backend probing caused a native startup
crash on the development machine. Other platforms use wgpu's primary backends.
A low-power adapter is requested; GPU tests selected the AMD Radeon 780M
integrated GPU. The adapter reports a limitation involving indirect-draw index
behavior, so future indirect drawing needs capability checks. The current path
uses direct indexed draws. Backend debug warnings may appear at shutdown.

## Build and run

Use a current stable Rust toolchain supporting edition 2024 (minimum language
version 1.85). On Windows, install the MSVC C++ build tools and Windows SDK.

Run commands from the project root:

```powershell
cargo run
cargo run --release
cargo run -- --preview
cargo run -- --offline
cargo run -- --preview --mesh assets/f-16_fighter_jet.glb
cargo run -- --smoke-test
cargo run -- --preview --smoke-test
```

`--mesh FILE.glb` selects the aircraft mesh. `--preview` enters local gameplay
directly. `--smoke-test` exits successfully after three presented frames.
These options can be combined. `--help` lists them.

`game.json` configures assets, window/frame rates, physics rate, flight tuning,
bot count, weapons, radar, and match rules. `game.local.json` overrides it locally;
`--config FILE.json` selects another file. `networking.json` configures the Python
runtime, listen addresses/ports, LAN discovery, connection limits/timeouts, and
advertised external addresses. Its local override is `networking.local.json`.
Normal network play needs Python 3.11+; offline preview does not start Python.

## User interface

All eleven screens from `UserInterface.md` are implemented: splash, start,
settings, browser, joining, hosting, waiting lobby, spawn, gameplay, pause, and
match report. The existing splash/menu PNGs are used; missing UI artwork falls
back to plain panels. An original bitmap font supplies placeholder text.

- Up/Down moves focus; Left/Right adjusts values or moves focus between buttons.
- Enter activates the focused control, like a left click. Mouse clicks work too.
- Click a `<` chevron to decrease a value; click the value or `>` to advance it.
- P opens the pause menu during gameplay and resumes from that menu.
- Escape goes back. Host Cancel and leaving a session return to the browser.
- Host name: Enter/click to edit, type ASCII text, Backspace to erase, Enter to finish.
- Settings opened from pause return to pause. Local gameplay pauses while menus are open.

The default browser discovers hosts on the LAN. Hosting and joining run through
the Python network service; there is no central directory or relay dependency.
Hosts automatically receive a `jetgame://` join link when they create a lobby.
Copy Join Link copies it; hosts never need to type a link or address. Paste it into the browser
and select Connect, or pass it with `--connect`. On Windows Ctrl+V pastes and
Ctrl+C in a hosting lobby copies the invitation. Its label distinguishes Internet
address candidates, the same LAN, and this computer only. Advanced forwarded IPv4
addresses can still be supplied through networking configuration or the CLI.
Connections attempt IPv6 before IPv4. Router/firewall reachability is still required.
Lobbies synchronize participants and Ready state. Network match start deliberately
reports that gameplay synchronization is not implemented yet. Use `--offline`
for the original mock joining flow and local bot matches, or `--preview` to fly
directly. Respawn, End Match, and Simulate Link Loss exercise the local round.

Host map/mode/player-limit choices are reflected in the lobby. They do not yet
select a different local game mode or level. All four documented control schemes
now operate the aircraft. Master Volume controls synthesized radar warnings;
Music Volume is reserved for a future music player. VSync applies immediately.
Settings are not persisted yet. Offline roster entries are placeholders; gameplay HUD
speed, altitude, health, ammunition, radar state, and round statistics are live.

The network worker delivers actual connection/lobby/disconnect events to the UI.
Future simulation messages can deliver match start/end and aircraft destruction.
A connection-loss event returns to the browser and clears the session.

UI rendering uses one alpha-blended instanced draw call, a 1024x1024 RGBA atlas
(4 MiB GPU allocation), and a reusable quad buffer. Images are reduced before
upload; their original decoded pixels are released. A retained display list is
rebuilt/uploaded only after a UI change. Full-screen menus skip the 3D draw and
depth clear, and unchanged menus do not redraw. Small timer updates run at 30 Hz;
the current gameplay redraw loop targets 120 Hz. Resolution-independent layout
and hit testing share a letterboxed 1280x720 logical canvas.

## Local gameplay

Every aircraft starts with guns selected, 800 gun rounds, and 8 missiles. Spawn
does not select a weapon. F switches guns/missiles; Space fires. Gun cadence is
12 shots/second for the player; missile cooldown is 0.9 seconds. Switching weapons
does not bypass the current cooldown. Respawning restores both weapons and ammo.

Radar scans automatically, independently of weapon selection. An enemy within
2,800 m and the 30-degree acquisition half-cone must remain visible for 1.25 seconds.
Tracking then permits a 45-degree half-cone. Terrain and placeholder cover block
line of sight. Range, angle, occlusion, death, or a different target life break
the lock. Both pilots have per-aircraft outgoing/incoming lock state; the local
pilot hears different lock and missile-warning tones. There is no remote audio
client until networking is implemented.

Missiles can fire with no lock and then fly unguided. A shot launched with a
lock receives a target and homes independently after the launching radar loses
lock. Invalid/dead targets stop guidance. Missiles turn at a limited rate, can
hit terrain, and expire; they are not guaranteed hits.

The local round has three simple opponents, health/damage, terrain/cover/aircraft
collision, impacts/explosions, respawning, and live results. First to five player
kills or three minutes ends the round. Opponents respawn after five seconds.
Crossing the combat boundary gives eight seconds to return. P/Escape pauses;
losing focus pauses and clears held input. Mouse schemes capture the cursor;
opening a menu releases it. The mouse virtual joystick remains sticky during
flight, and simple mouse steering honors manual yaw/roll overrides.

Physics uses a local six-degree-of-freedom rigid body with mass, diagonal inertia,
quaternion orientation, thrust, gravity, lift, drag, and control torque. Automatic
lift trim, side-slip damping, coordinated banking, and softened stall/control
authority make it forgiving. Lift uses the standard
[dynamic-pressure lift relation](https://www1.grc.nasa.gov/beginners-guide-to-aeronautics/lift-equation-2/);
the coefficients and assistance are gameplay tuning rather than a calibrated F-16
simulation. Integration background: [rigid-body physics](https://gafferongames.com/post/physics_in_3d/).
`flight::FlightTuning` contains the main tuning values. Simulation runs at a fixed
120 Hz with a bounded catch-up step, independently of rendering cadence.

Rendering interpolates the last two physics snapshots using the fractional tick
remainder: linear position blending and quaternion spherical interpolation for
orientation. Aircraft, projectile visuals, the chase camera, and target markers
use the same presentation timeline. Physics, collision, radar, and damage use
authoritative fixed-step state. This adds one physics tick (about 8.3 ms) of visual
latency rather than extrapolating. Spawn/teleport snapshots reset to prevent streaking.

The supplied arbitrary terrain mesh is scaled to a 12 km arena with reduced
vertical relief. Its triangles form a CPU BVH for collision and radar occlusion;
this is not a heightfield. Aircraft collision uses forgiving sphere proxies.
Projectiles sweep segments to avoid tunneling. `rocket.glb` is reused as 72 tree
and rock placeholders with shared geometry and simple collision proxies.
Projectile and explosion counts are bounded. Consecutive objects sharing a mesh
are submitted as one instanced draw; terrain, cover, and aircraft normally need
three world draws, plus projectile/effect batches and one UI draw. Procedural
audio streams tones without decoding or retaining audio clips.

## Object loading and submission

`Renderer::load_mesh(path)` resolves a file address into a reusable `MeshHandle`.
The loader decodes the default GLB scene (or first scene), combines its rigid
mesh primitives, and applies nested node transforms. It preserves supplied
normals and generates averaged normals when absent. Mirrored node transforms
have their triangle winding corrected. Triangle lists, strips, and fans are
converted to the engine's indexed triangle layout.

A canonical file path is loaded only once per mesh store. Multiple objects can
share its GPU vertex/index buffers while using different transforms. CPU decoding
data is released after loading. Handles identify resources within the owning
renderer; resolve configured asset paths during startup rather than persisting
handle numbers between runs. No raw GPU address is exposed.

Example, after creating a renderer:

```rust,ignore
use glam::{Mat4, Vec3};
use jetgame::rendering::RenderObject;

let rocket = renderer.load_mesh("assets/rocket.glb")?;
renderer.scene.objects.push(RenderObject {
    mesh: rocket,
    transform: Mat4::from_translation(Vec3::new(5.0, 2.0, 0.0)),
});
renderer.scene.material.shininess = 64.0;
```

Object transforms support rotation, translation, and nonuniform positive scale.
Runtime transforms must be finite, affine, nonsingular, and preserve handedness;
reflections in GLB nodes are baked correctly by the loader. Normals use inverse
transpose transforms. The scene exposes camera, light, and shared material
parameters. The material's RGB color is linear, with configurable ambient
strength, specular strength, and shininess.

GLB is a temporary decoder, separate from GPU storage. A future packed-file
decoder can produce `MeshData` (position/normal vertices and `u32` indices) and
call `Renderer::upload_mesh(label, data)` without changing the forward pipeline.
This version expects embedded GLB geometry buffers and rejects skins, morph
targets, line/point primitives, and invalid geometry with errors. Animation and
texture decoding for 3D materials are not implemented. UI PNG decoding is separate.

## Source layout

```text
src/
  main.rs                       Command-line options and logging
  app.rs                        Window events and startup mesh selection
  ui.rs                         Screens, widgets, navigation, session events
  config.rs                     Game configuration and validation
  flight.rs                     Rigid body, force model, four control schemes
  terrain.rs                    Static triangle BVH and collision queries
  game.rs                       Fixed simulation ticks, bots, radar, combat
  game/presentation.rs          Mesh instances, camera, HUD projection
  audio.rs                      Procedural lock / incoming-missile cues
  lib.rs                        Engine entry modules
  networking/                   Python launcher and typed local pipe interface
  rendering/
    mod.rs                      Renderer, scene ownership, load/upload API
    context.rs                  Device, queue, surface, resize, depth target
    mesh.rs                     GLB decoder, mesh layout, cached GPU handles
    forward.rs                  Camera, shared material, forward draw pipeline
    blinn_phong.wgsl            Universal vertex and fragment shader
    frame.rs                    CPU frame timing
    shadow_bake.rs              Reserved one-time level preprocessing
    ui_gpu.rs                   Cached atlas and batched unlit UI rendering
    ui.wgsl                     Screen-space overlay shader
tests/
  forward_gpu.rs                GPU readback and depth/caching verification
  ui_gpu.rs                     All-screen visual previews and keyboard route
  networking.rs                 Rust/Python command/event integration
networking/                     Python discovery, invitations, lobby and transports
game.json                       Editable game/assets/physics/match policy
networking.json                 Python runtime and network policy
assets/                         Supplied models, images, and audio
```

There are no separate jet, terrain, or ground-cover renderer modules. Future
specialized optimizations can use the shared mesh path until measurements justify
a different approach. Current combat visuals use shared bullet/missile meshes and
small placeholder explosion instances; a richer particle system remains future work.

## Verification

```powershell
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo test --test forward_gpu -- --ignored --nocapture
cargo test --test ui_gpu -- --ignored --nocapture
cargo test --test networking -- --ignored --nocapture
python -m unittest discover -s networking/tests -v
```

The explicit GPU test loads all four supplied GLBs, confirms cached mesh handles,
renders each to an offscreen texture, and reads pixels back to verify geometry
and lighting. It also checks depth occlusion with overlapping objects in both
submission orders. It writes inspectable PNG previews to
`target/render-previews/`. The GPU test is ignored by the default test command
because it requires a graphics adapter.

CPU UI tests cover keyboard navigation, scaled mouse clicks, disabled actions,
settings, text editing, host cancellation, pause/resume, and global disconnection.
The UI GPU test exercises all eleven screens through keyboard actions, checks
their rendering, and writes PNG previews to `target/ui-previews/`. Both native
startup and direct gameplay modes have three-frame smoke tests. Input behavior
is covered by deterministic controller tests.

Physics/combat tests cover neutral flight, maneuver stability, control signs,
fixed-step behavior, collision sweeps, damage/ammo/respawn, radar visibility and
warnings, cooldowns, independent missile guidance, and bounded audio samples.
The UI GPU suite also renders a live flight scene and radar/missile warning scene.

## Remaining design work

Terrain shadow volumes remain a one-time baking task. The forward shader does
not apply baked shadows yet. Aircraft shadows, textured/per-object materials,
richer effects, asset streaming, recorded sound effects/music, networked gameplay,
and further gameplay tuning remain. Clouds are outside immediate scope.

Presentation defaults to `AutoNoVsync` with the backend's supported fallback;
VSync is selectable in Settings. Gameplay is paced toward 120 redraws per second.
The working goals remain 2K at 120 FPS and approximately 512 MB
to 1 GB of memory. The current tests verify correctness; they do not demonstrate
those performance targets. Exact benchmark resolution and memory accounting
remain to be settled.

See `graphics_pipeline_discussion_summary.md`, `Controls.md`,
`UserInterface.md`, and `checklist.md` for the design requirements.
