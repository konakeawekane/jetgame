# Source organization

| Responsibility | Location |
| --- | --- |
| Window, input, scheduling, service coordination | `src/app.rs` |
| Game/asset configuration and validation | `src/config.rs`, `game.json` |
| CPU aircraft physics and control conversion | `src/flight.rs` |
| Match state, fixed ticks, radar, combat, local bots | `src/game.rs` |
| GPU asset instances, camera, HUD projection | `src/game/presentation.rs` |
| Host ticks, input history, prediction and interpolation | `src/game/multiplayer.rs` |
| Quantized world export/import and actor lifecycle | `src/game/replication.rs` |
| Typed game session/input/state messages | `src/game/net_types.rs` |
| Static collision geometry | `src/terrain.rs` |
| Menu/session state and lightweight display lists | `src/ui.rs` |
| GPU resources and rendering | `src/rendering/` |
| Typed application network commands/events | `src/networking/mod.rs` |
| Owned Python process and bounded pipe IPC | `src/networking/bridge.rs` |
| Network configuration and connection/discovery policy | `networking/jetgame_net/` |
| UI-independent headless host entry point | `networking/server_main.py` |

`game.json` contains editable game policy: assets, frame rates, physics rate,
flight tuning, loadouts, radar rules, match limits, bot/cover counts, and cover
seed. Defaults preserve the existing preview. Numerical algorithm constants and
UI geometry stay with their implementations. Local overrides are ignored by Git.

The simulation owns authoritative poses; rendering interpolates previous/current
snapshots. `World::step_with_controls` accepts a per-aircraft list of optional
`Controls` and performs one fixed tick. Supplied controls override local bots.
This entry point does not create a window, GPU, or network process. `advance`
adapts local input and elapsed render time to those ticks. The presentation module
reads world state and cannot advance simulation.

Python's `Lobby` model is independent of sockets and the UI. `MatchServer` adapts
it to framed TCP; `Connection` is the client adapter. `NetworkService` adds
application lifecycle and emits plain events; `agent_main.py` handles pipe IPC.
The standalone host reuses the same server/model. A dedicated deployment can
therefore replace orchestration without rewriting discovery or lobby transport.

`Multiplayer` coordinates the same fixed-tick world for an authoritative host or a
predicting replica. Slot assignments remain stable across joins/leaves; input
sequence acknowledgements and match epochs reject duplicates/old-round traffic.
`replication.rs` converts poses and game state to bounded integer rows. Python's
`realtime.py` owns UDP authentication, RLE input chunks, state compression, and
per-client packet budgets. This keeps transport out of the Rust game.

Full dedicated flight hosting still needs a headless Rust runner calling this
existing simulation adapter and the same Python command/event interface. The
standalone Python entry point alone is a lobby host; it does not implement plane
physics. Lag compensation and encrypted control transport remain future work.
[Networking details](networking/README.md) describe current behavior and limits.
