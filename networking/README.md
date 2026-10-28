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
instance can join, change Ready state, and leave. Network flight replication is
still pending; Start Match returns an explicit not-implemented response.
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
For example, forward external TCP 40000 to this computer's TCP 29010, then configure
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
as a game-hosted session. No Rust window/GPU is required to run this lobby host.

`Lobby` owns participant/permission state; `server.py` owns listener/session
lifecycle; `client.py` owns client sessions and address fallback; `discovery.py`
owns LAN queries; `links.py` owns invitation parsing/generation. `NetworkService`
adapts those components to application commands. Future gameplay traffic should
extend this service while retaining Rust simulation, rather than duplicating the
flight model in Python. See [architecture.md](../architecture.md).

## Protocol and lifecycle

Network control frames use a four-byte big-endian length followed by UTF-8 JSON,
at most 65,536 bytes. Version 1 envelopes contain exactly `version`, `kind`,
`request_id`, and object `payload`. The lobby supports join/welcome, roster,
ready, ping/pong, leave, and an explicitly unsupported match-start request.
TCP is selected for lobby control only; real-time gameplay transport is undecided.

Application IPC uses JSON lines capped at 65,536 bytes, bounded input/output
queues, and operation IDs. New operations/canceling a join cancel pending work;
stale events cannot reopen old screens. Owner credentials stay inside the Python
host process and its loopback owner connection. Leaving closes owned sessions
and listeners; closing the app closes its agent's stdin and ends the agent.
Rust kills only its owned child if shutdown stalls.

Listings, links, and remote messages are untrusted. Frame/schema limits, deadlines,
connection/request caps, and roster validation reduce malformed-input/resource
risks. The current transport is plaintext and is not a production security
solution: there is no authenticated encryption, match password feature, or anti-cheat
yet. A malicious host can lie about a match. No received code/files are executed.

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
framing, roster/permissions, full lobbies, pipe IPC, and shutdown. Multicast tests
need permission to use LAN sockets; a loopback-only sandbox will block them.
Cross-network Internet/CGNAT behavior needs testing on separate real networks.
