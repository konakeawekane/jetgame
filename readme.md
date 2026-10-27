# JetGame

Rust engine for a low-poly multiplayer jet dogfighting game, using `wgpu 0.19`
and `winit 0.29`. Python networking is planned separately.

## Current rendering path

A single forward pipeline draws every rigid mesh using one universal opaque
Blinn-Phong material. It supplies ambient light, directional diffuse light, and
a specular highlight. Mesh position/normal data and object transforms feed the
same shader for jets, terrain, and other standard 3D objects.

The application opens a resizable 1280x720 development window and displays the
supplied F-16 model. The camera automatically frames the loaded geometry without
changing its scale. Depth testing and back-face culling are enabled. All meshes
receive the same gray material; embedded material assignments, textures, vertex
colors, and HDR skies are deliberately unused at this stage.

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
cargo run -- --mesh assets/world_01.glb
cargo run -- --mesh assets/rocket.glb
cargo run -- --smoke-test
```

`--mesh FILE.glb` selects the displayed mesh. `--smoke-test` exits successfully
after three presented frames. These options can be combined. `--help` lists them.

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
texture decoding are not implemented.

## Source layout

```text
src/
  main.rs                       Command-line options and logging
  app.rs                        Window events and startup mesh selection
  lib.rs                        Engine entry modules
  rendering/
    mod.rs                      Renderer, scene ownership, load/upload API
    context.rs                  Device, queue, surface, resize, depth target
    mesh.rs                     GLB decoder, mesh layout, cached GPU handles
    forward.rs                  Camera, shared material, forward draw pipeline
    blinn_phong.wgsl            Universal vertex and fragment shader
    frame.rs                    CPU frame timing
    shadow_bake.rs              Reserved one-time level preprocessing
tests/
  forward_gpu.rs                GPU readback and depth/caching verification
assets/                         Supplied models, images, and audio
```

There are no separate jet, terrain, or ground-cover renderer modules. Future
specialized optimizations can use the shared mesh path until measurements justify
a different approach. Dynamic combat effects remain future work.

## Verification

```powershell
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo test --test forward_gpu -- --ignored --nocapture
```

The explicit GPU test loads all four supplied GLBs, confirms cached mesh handles,
renders each to an offscreen texture, and reads pixels back to verify geometry
and lighting. It also checks depth occlusion with overlapping objects in both
submission orders. It writes inspectable PNG previews to
`target/render-previews/`. The GPU test is ignored by the default test command
because it requires a graphics adapter.

## Remaining design work

Terrain shadow volumes remain a one-time baking task. The forward shader does
not apply baked shadows yet. Aircraft shadows, combat effects, asset streaming,
UI, and gameplay still need implementation. Clouds are outside immediate scope.

Presentation uses `AutoNoVsync` with the backend's supported fallback and has no
FPS limiter yet. The working goals remain 2K at 120 FPS and approximately 512 MB
to 1 GB of memory. The current tests verify correctness; they do not demonstrate
those performance targets. Exact benchmark resolution and memory accounting
remain to be settled.

See `graphics_pipeline_discussion_summary.md`, `Controls.md`,
`UserInterface.md`, and `checklist.md` for the design requirements.
