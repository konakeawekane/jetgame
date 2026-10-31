# Networking

Rust owns gameplay/rendering/UI. Python owns LAN discovery, Internet connections,
hosting, framing, and lobby state. The Rust bridge only launches an owned process
and exchanges bounded JSON lines over stdin/stdout; it has no network sockets.
The Python code uses the standard library and requires Python 3.11+.

## Play and host

```powershell
cargo run
```

Play opens the LAN browser. Refresh queries local hosts; select one and Join.
Host Match -> Create Lobby starts a Python host and connects its owner. Another
instance can join and mark Ready. Once everyone is ready the host starts the
round; each player chooses a team and spawns. Players can join a running round.
The host owns simulation, bots, radar, damage, ammunition, respawns, and results.
Only the host may end a round. Its report's Continue returns everyone to the lobby.
Local bot flight remains available with `cargo run -- --preview`; `--offline`
retains the mock menu flow. Neither offline mode launches Python.

Creating a host generates a validated, versioned `jetgame://join?...` link
automatically from its active listeners and detected addresses. Hosts do not
enter an address or construct a link. Copy Join Link in the lobby copies it;
the scope label distinguishes Internet address candidates, the same LAN, and
this computer only. Paste it in the browser and choose Connect. On Windows Ctrl+V pastes and
Ctrl+C in the lobby copies. The link is text to paste; installation does not
register an operating-system URL handler.

```powershell
cargo run -- --connect 'jetgame://join?v=1&endpoint=example.net%3A29010'
cargo run -- --connect '[::1]:29010'
cargo run -- --public-address example.net:40000
```

The example domain is a placeholder. Quote links in shells because they contain
`&`. Host-generated links include a match ID and up to eight addresses. A matching
ID prevents accidental connection to a different match; it is not cryptographic
authentication or a password. Links are not secrets and contain host addresses.

## Connectivity without infrastructure

Discovery is IPv4 LAN multicast with TTL 1, a nonce, bounded responses, and a
configurable group/port/interface. The default interface selection tries the OS
default and OS-reported local IPv4 addresses. Multicast is best effort; guest Wi-Fi,
VPNs, firewalls, or separate subnets can block it. Pure IPv6 LANs can use invitations
or direct addresses; IPv6 multicast discovery is not implemented.

TCP listening supports separate IPv6 and IPv4 sockets. Client resolution orders
IPv6 candidates before IPv4 and tries the remaining candidates after connection
or handshake failure, with bounded per-attempt timeouts. This is sequential
fallback, not a full Happy Eyeballs implementation.

Invitations automatically include OS-reported globally routable IPv6 and direct
public IPv4 candidates when available, plus LAN fallback addresses. Failed listen
families are excluded. IPv6 still requires an inbound firewall path. When no global
candidate or configured public address exists, the UI labels the link for the
same LAN, or this computer only if no LAN address is available.
The OS may not report every interface address; an explicit IPv6 address can be supplied.
An invitation's external address is an unverified candidate, not proof of reachability.

Behind IPv4 NAT, supply the router's public address and forwarded external port.
For example, forward external **TCP and UDP** 40000 to this computer's TCP and UDP
29010, then configure
`YOUR_PUBLIC_ADDRESS:40000` through `public_addresses` or `--public-address`.
This is an advanced override; there is no address field in the host menu.
The game cannot infer a router's public IPv4 address
from its private address without outside assistance, so it does not guess one.
No public-IP lookup, STUN, directory, relay, automatic port mapping, or firewall
changes occur. CGNAT or blocked incoming traffic can prevent external hosting.

## Configuration

`networking.local.json` overrides `networking.json`. Rust reads launcher fields
`python` and `agent_script`; all remaining fields go to the single Python
`NetworkSettings` schema in `jetgame_net/config.py`. Unknown settings and invalid
limits fail explicitly. Set `listen_ipv6` or `listen_ipv4` to an empty string to
disable that listener. Set `lan_enabled` false for invitation-only hosting.
`public_addresses` is a list of complete external addresses including ports.
`--host-port` overrides the listen port; `--public-address` can be repeated.

Debug builds fall back to Python on PATH. `--python PATH`, `JETGAME_PYTHON`, or
the `python` field select an interpreter. `--server-python` and
`JETGAME_SERVER_PYTHON` remain interpreter aliases. Release packages require a
bundled or explicitly configured interpreter for networking, including joining.
Offline preview remains independent of Python.

## Standalone hosting and extension

```powershell
python networking/server_main.py --config networking.json --name 'MY MATCH' --max-players 8
python networking/server_main.py --port 29010 --no-lan --public-address example.net:40000
```

The standalone process prints readiness and its invitation, then runs until
interrupted. It uses the same `MatchServer` and transport-independent `Lobby`
as a game-hosted session. No Rust window/GPU is required to run this lobby host;
full dedicated flight requires a headless Rust authority runner using the same
simulation adapter (the Python server does not reimplement plane physics).

`Lobby` owns participant/permission state; `server.py` owns listener/session
lifecycle; `client.py` owns client sessions and address fallback; `discovery.py`
owns LAN queries; `links.py` owns invitation parsing/generation. `NetworkService`
adapts those components to application commands. `realtime.py` handles gameplay
UDP; Rust's `game/multiplayer.rs` coordinates the authoritative or predicting
simulation. See [architecture.md](../architecture.md).

## Gameplay synchronization

The host's fixed-rate Rust simulation is authoritative. The default physics rate
is 120 Hz, independent of render frequency and incoming packet rate. Online
simulation continues through pause/settings/spawn menus, focus loss, and an
occluded window. Menus clear local controls rather than pausing other players.

Clients sample controls at the physics rate and send input chunks at **20 Hz**.
By default a chunk contains six new samples plus twelve recent samples for loss
recovery. Python run-length encodes repeated controls; axes use signed thousandths,
and fire/weapon selection are absolute states. Sequence numbers deduplicate input.
The host uses at most one sample per player per physics tick, buffers jitter,
drops excessive backlog, and stops stale fire. It never accepts client positions,
damage, ammunition, or client-chosen player IDs.

The host exports state at **20 Hz** through local IPC. Python sends an individualized
UDP packet to each guest, always including that guest's aircraft and its consumed
input acknowledgement. Four other aircraft rotate through the packet by default;
health/life changes take priority over ordinary rotation.
Poses and game state are quantized integer rows; packets are compressed and kept
within **1,200 bytes of UDP payload**, including the authentication header/tag.
The host's local input does not take a round trip through UDP.

Projectile updates prioritize threats to the recipient and new shots. Gun spawns
are repeated, missiles continue receiving corrections, explosions are repeated,
and removal tombstones persist for two seconds. Lower-priority visuals may be
delayed or omitted under a saturated packet budget; authoritative collision and
damage still occur on the host. This is state trickling, not a full-world
broadcast every rendered frame.

Guests predict only their own rigid-body flight, restore the host pose, discard
acknowledged inputs, and replay unacknowledged inputs. Visual correction offsets
decay smoothly; spawn/death discontinuities snap. Other jets interpolate buffered
poses, use bounded dead reckoning between sparse updates, and smooth corrections.
Health, weapons, radar warnings, and round results always come from the host.
There is no client-side damage authority or lag-compensated hit rewind yet.

`game.json`'s `sync` object configures batching, redundancy, jitter buffering,
snapshot frequency, entity chunks, interpolation/extrapolation, histories, and
packet budgets. Online projectiles are capped at 256 and actors at 47 (up to 32
human slots plus 15 bots). The host sends physics/match settings at round start;
clients verify their collision-level fingerprint before accepting the session.
No remote asset paths or executable content are loaded.

TCP reliably carries match preparation/start, slot changes, spawn/respawn actions,
action results, round return, and lobby messages. Both TCP and UDP use the same
externally visible invitation port, including when a router maps that port to a
different internal port. If gameplay UDP remains unavailable, the guest times out
with a UDP/firewall message rather than silently playing an unsynchronized game.

Each round has a new epoch and each player has a random UDP key negotiated over
TCP. Datagram authentication rejects off-path spoofing and wrong-round packets;
local IPC also checks epochs so queued inputs/actions cannot leak into a restart.
Late joins receive the current setup and stable slot assignments, then receive
their own state immediately and other aircraft through the regular trickle.

## Protocol and lifecycle

Network control frames use a four-byte big-endian length followed by UTF-8 JSON,
at most 65,536 bytes. Version 1 envelopes contain exactly `version`, `kind`,
`request_id`, and object `payload`. The lobby supports join/welcome, roster,
ready, ping/pong, leave, match preparation/start, actions, and round return.
Gameplay uses a separate bounded UDP channel on the same numeric port.

Application IPC uses JSON lines capped at 65,536 bytes, bounded input/output
queues, and operation IDs. New operations/canceling a join cancel pending work;
stale events cannot reopen old screens. Owner credentials stay inside the Python
host process and its loopback owner connection. Leaving closes owned sessions
and listeners; closing the app closes its agent's stdin and ends the agent.
Rust kills only its owned child if shutdown stalls.

Listings, links, and remote messages are untrusted. Frame/schema limits, deadlines,
connection/request caps, and roster validation reduce malformed-input/resource
risks. The current transport is plaintext and is not a production security
solution: UDP HMAC tags do not replace authenticated encryption of the TCP key
exchange. There is no match password feature or protection from a malicious host
or on-path attacker yet. No received code/files are executed.

## Packaging and checks

```powershell
.\scripts\package.ps1 -PythonRuntime C:\SDKs\python-embed
python -m unittest discover -s networking/tests -v
cargo test
cargo test --test networking -- --ignored --nocapture
```

The packaging script copies the Rust executable, assets, both configuration files,
Python services, and a supplied Windows embeddable runtime into a new `dist/`
folder. No directory endpoint is needed. An embedded Python distribution is not
vendored; full packaging still needs testing with the selected distribution.

Tests cover LAN discovery between local hosts, IPv6, IPv4 fallback, strict links,
framing, roster/permissions, full lobbies, pipe IPC, UDP/RLE/MTU budgets, spoof
rejection, prediction, authoritative movement/fire/health/ammo, late joining,
round restart, and shutdown. Multicast tests
need permission to use LAN sockets; a loopback-only sandbox will block them.
Cross-network Internet/CGNAT behavior needs testing on separate real networks.
