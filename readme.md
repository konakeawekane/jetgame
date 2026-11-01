# JetGame

A lightweight multiplayer jet dogfighting game. Rust owns gameplay, physics,
rendering, and UI; Python 3.11+ owns discovery, connections, and networking.

## Run and test multiplayer

Install stable Rust (edition 2024, version 1.85+). Prepare the private runtime once
with `.\scripts\setup-runtime.ps1`; no Python installation is needed.
Windows packages target Windows 10/11 x64. Players should download the release
ZIP, extract the entire folder, and launch `jetgame.exe` inside it. The private
Python DLLs must stay in `runtime/python`; an EXE copied out of the folder is
not a complete game. Release launches have no console or routine terminal logs.

Windows builds require the MSVC C++ build tools and Windows SDK.
Run from the project root in two terminals:

```powershell
cargo run --offline
```

Cargo's `--offline` only prevents dependency downloads; the game starts online.

1. In the first instance: Play -> Host Match -> Create Lobby.
2. In the second: Play -> Refresh -> select the host -> Join Selected.
   Alternatively, paste the host's generated invitation and choose Connect.
3. Mark the guest Ready, then select Start Match on the host.
4. Select opposing teams (or host Free For All) and Spawn in both instances.

Only actual discovered hosts appear. Connections and match starts wait for server
messages. Networking failures display an error instead of starting local gameplay.
The lobby shows connected pilots. Bots default to zero.

```powershell
cargo run --offline -- --preview
cargo run --offline -- --connect '127.0.0.1:29010'
cargo run --offline -- --help
```

`--preview` explicitly starts local flight without Python and displays OFFLINE
PREVIEW. Leaving preview exits. Set `rules.bot_count` in a chosen configuration
if you want bots. The old game `--offline` flag and simulated server menus have
been removed. Use `cargo run --offline -- --preview`, not a trailing `--offline`.
`--smoke-test` exits after three presented frames.

## Controls and configuration

Arrows navigate menus; Enter activates the selected control. P opens/resumes the
pause menu. F changes guns/missiles; Space fires. See [Controls.md](Controls.md).
Online menus and focus loss clear local input while the shared simulation runs.
Leaving or losing the server returns to the browser.

`game.json` configures assets, flight, bots, match rules, rendering rates, and
synchronization. `game.local.json` replaces it locally; `--config FILE.json`
selects another file. `networking.json` configures Python and network policy;
`networking.local.json` replaces it locally. Local files are ignored by Git.

The host controls movement, radar, combat, damage, spawning, and match results.
Clients send input chunks, predict local flight, reconcile to the host, and
interpolate remote aircraft. See [networking/README.md](networking/README.md)
for Internet hosting, synchronization, runtime packaging, and current limits.

## Source map

| Responsibility | Source |
| --- | --- |
| Startup, window, input, scheduling | `src/main.rs`, `src/app.rs` |
| Configuration, menus | `src/config.rs`, `src/ui.rs` |
| Flight, fixed ticks, combat, radar, bots | `src/flight.rs`, `src/game.rs` |
| Session authority, prediction, interpolation | `src/game/multiplayer.rs` |
| Quantized replication and actor lifecycle | `src/game/replication.rs`, `src/game/net_types.rs` |
| Scene instances, camera, HUD | `src/game/presentation.rs` |
| Terrain collision BVH, procedural audio | `src/terrain.rs`, `src/audio.rs` |
| Mesh loading, forward Blinn-Phong, UI overlay | `src/rendering/` |
| Typed local IPC and owned Python worker | `src/networking/` |
| Discovery, invitation links, TCP/UDP transport | `networking/jetgame_net/` |

The shared renderer caches GLB geometry and uses instanced opaque meshes with a
universal gray material. UI uses a batched unlit atlas. Physics is independent of
rendering. Supplied terrain is arbitrary triangle geometry; missiles currently
provide placeholder cover meshes. Textured materials, shadows, richer effects,
settings persistence, and lag compensation remain unfinished. Clouds are outside
immediate scope. The targets of 2K/120 FPS and 512 MB-1 GB memory remain unverified.

## Checks

```powershell
cargo fmt --all -- --check
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
.\runtime\python\python.exe -m unittest discover -s networking/tests -v
cargo test --offline -- --ignored --nocapture
```

Ignored tests require Python/local sockets or a GPU. GPU previews are written to
`target/render-previews/` and `target/ui-previews/`.

Design requirements remain in [UserInterface.md](UserInterface.md),
[Controls.md](Controls.md), [checklist.md](checklist.md), and
[graphics_pipeline_discussion_summary.md](graphics_pipeline_discussion_summary.md).
