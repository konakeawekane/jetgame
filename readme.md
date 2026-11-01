# JetGame

JetGame is a lightweight multiplayer jet dogfighting game. Rust handles rendering, flight physics, combat, and menus. A bundled Python process handles LAN discovery, invitation links, TCP lobby messages, and UDP gameplay updates. The host controls the simulation; clients batch inputs and interpolate synchronized state.

## Instructions for Build and Use

Steps to build and/or run the software:

1. Install Rust/Cargo and the MSVC C++ build tools with the Windows SDK.
2. From the project root, run `.\scripts\setup-runtime.ps1` to prepare the private Python runtime. No global Python installation is required.
3. Run `cargo run` for debug or `cargo run -r` for release. Build without launching using `cargo build --release --locked`.

Create a distributable with `.\scripts\package.ps1`. Share its ZIP; players must extract the entire folder before launching `jetgame.exe`.

Instructions for using the software:

1. Host: **Play > Host Match > Create Lobby**.
2. Guest: **Play > Refresh > select host > Join Selected**, or paste an invitation and connect.
3. Guests select **Ready**; the host starts the match. Choose opposing teams and spawn.

Arrows/Enter navigate menus; **P** pauses, **F** switches weapons, and **Space** fires. See [Controls.md](Controls.md). Internet hosting requires reachable IPv6 or IPv4 forwarding for TCP and UDP, normally port **29010**. See [networking details](networking/README.md).

## Development Environment

Development versions and requirements:

* Windows 10/11 x64 and PowerShell 5.1 or newer.
* Rust/Cargo **1.98.1**, used during development; Rust edition **2024**.
* Bundled CPython **3.14.10**; networking uses its standard library.
* `wgpu` **0.19.4**, `winit` **0.29.15**, and other dependencies declared in `Cargo.toml`; `Cargo.lock` records exact versions.
* Editable game/network settings: `game.json` and `networking.json`.

## Useful Websites to Learn More

Networking references:

* [Python sockets](https://docs.python.org/3/library/socket.html): TCP/UDP sockets and address families.
* [Asyncio streams](https://docs.python.org/3/library/asyncio-stream.html): asynchronous TCP connections.
* [Asyncio protocols](https://docs.python.org/3/library/asyncio-protocol.html): UDP datagram transports.
* [Snapshot interpolation](https://gafferongames.com/post/snapshot_interpolation/): smoothing network updates.

## Future Work

[ ] Improving Gameplay to be more fun
[ ] Add Textures, Emissive Shaders, Particle FX, Shadows, Sky shaders
[ ] Improved Terrain model and assets
[ ] Improved UI quality