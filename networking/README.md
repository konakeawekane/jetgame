# Networking scaffold

This is a working lobby/control-plane prototype, not synchronized multiplayer
flight and not a production-ready Internet service. Python runs only headless
servers: a directory service and player-hosted match servers. The client is Rust.

## Architecture

```text
Rust client ------ shared public Python directory ------ hosted match listings
     |
     +----------- Python match server (on a player's PC or dedicated host)

Host button ---- owned Python server process bundled with that game copy
```

Every package includes hosting and the same configurable directory endpoint.
Joining/discovery does not start Python. Hosting does; its owner also connects as
an ordinary Rust lobby client with a one-time owner credential. A new public
directory instance on each player's PC would NOT produce global discovery:
clients and hosts need a common, Internet-reachable directory deployment.

## Run locally

Python 3.11+ is enough; there are no Python package dependencies.

Terminal 1, start the shared development directory:

```powershell
python networking/server_main.py --service directory --bind 127.0.0.1 --port 29000
```

Terminal 2, run the native game browser/host UI:

```powershell
cargo run -- --directory 127.0.0.1:29000
```

Host Match -> Create Lobby starts the local Python lobby. A second game instance
using the same directory can Refresh, select the server, and Join. Roster and
Ready updates are real. Start Match returns an explicit not-implemented response;
the application does not pretend its local bots are remote multiplayer players.

For a standalone match server or direct connection:

```powershell
python networking/server_main.py --bind 127.0.0.1 --port 29010
cargo run -- --connect 127.0.0.1:29010
```

Use `cargo run -- --offline` for the original mock session flow, or `--preview`
for direct local flight. Neither starts a networking worker/server.

## Configuration and distribution

`networking.local.json` (ignored) overrides `networking.json`. Environment
`JETGAME_DIRECTORY` / `JETGAME_SERVER_PYTHON` and CLI `--directory` /
`--server-python` override those values. `--host-port` chooses a stable listen
port; `--public-port` overrides the advertised router-forwarded port.

The packaged root contains `jetgame.exe`, `assets/`, `networking/`,
`networking.json`, and `runtime/python/`. See `runtime/README.md` in the source.
Windows packaging requires both an embedded runtime and directory endpoint:

```powershell
.\scripts\package.ps1 -PythonRuntime C:\SDKs\python-embed -DirectoryEndpoint directory.example.com:29000
```

The example hostname is not a deployed service. Supply a real operator-controlled
endpoint. The script builds/copies into a new `dist/` directory, never deletes an
existing package, and does not download/install Python or publish anything.
`-Development` permits a loopback directory. Packaging code is scaffolded; a
complete bundle still needs testing with the chosen Python distribution.

## Protocol and lifecycle

- TCP, a four-byte big-endian byte length, then UTF-8 JSON; maximum body 65,536 bytes.
- Envelope: `version: 1`, `kind`, unsigned `request_id`, object `payload`.
- Directory: `discover`, `register`, `heartbeat`, `unregister`.
- Lobby: `join` -> `welcome`, `lobby` roster events, `ready`, `ping`/`pong`, `leave`.
- `start_match` is reserved and explicitly rejected until gameplay authority/sync exists.
- Hosts receive a private directory lease token, heartbeat every five seconds, and
  expire after twenty seconds without renewal. The observed source address is
  used; listing endpoints are marked **unverified**, not proven connectable.
- Owner credentials and directory lease credentials are different. Guests cannot
  start matches or update/remove another host's listing.
- Rust uses a background worker, bounded request/event/send/receive queues,
  frame-size limits, connection/response deadlines, keepalives, and session
  operation IDs so cancelled/stale results cannot reopen old screens.
- Disconnects/timeouts return to the browser. Parent-process EOF shuts down the
  owned server; expiring leases cover abrupt host failures.

The initial directory is in memory and capped at 512 matches; clients fetch
pages of at most 100 listings. The initial lobby cap is 32 players. These are prototype limits,
not a claim of worldwide service scale.

## What is deliberately undecided

TCP here is for low-frequency control messages. It does not choose the eventual
UDP/QUIC/TCP real-time transport, reliability channels, tick/snapshot frequency,
authority, client prediction, reconciliation, lag compensation, or server-side
flight integration. The Rust physics should be shared/exposed to an authoritative
server rather than casually reimplemented differently in Python. The current
server has no physics, collision, combat, or snapshot replication.

## Internet hosting difficulty

Global discovery requires a deployed public directory, a stable endpoint/domain,
operations/availability, and a compatible directory configuration in every build.
Registration is not proof of external reachability. A public IP/IPv6 route or
manual port forwarding can support direct hosting; NAT/CGNAT and restrictive
firewalls can prevent direct incoming connections. Automatic hosting across
networks needs a considered NAT traversal and/or relay service. Relays require
public infrastructure and bandwidth; packaging a server alone cannot supply it.

[ICE](https://www.rfc-editor.org/rfc/rfc8445.html) describes connectivity checks
and NAT traversal; [TURN](https://www.rfc-editor.org/rfc/rfc8656.html) describes
relay operation where a direct path is unavailable. Those are architectural
references, not implemented dependencies or a commitment to either standard.

Current control traffic is plaintext, unauthenticated beyond scoped lease/owner
tokens, and suitable for trusted local development only. Before public deployment,
choose/add TLS or another authenticated transport, endpoint reachability checks,
admission/rate limits, credentials, and abuse handling. No firewall/router changes,
port mapping, cloud deployment, or public test traffic are performed by this scaffold.

## Tests

```powershell
python -m unittest discover -s networking/tests -v
cargo test
cargo test --test networking -- --ignored --nocapture
```

The explicit Rust/Python test starts local services on ephemeral loopback ports,
hosts, advertises, discovers, joins with a client that has no valid Python runtime,
updates Ready, rejects unimplemented gameplay, disconnects, and verifies process
and directory cleanup. Python tests cover framing, leases, capacity, permissions,
rosters, and parent-pipe shutdown. External Internet/NAT traversal is not tested.
