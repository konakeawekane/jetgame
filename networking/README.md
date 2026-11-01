# Networking

Python 3.11+ uses only its standard library. Rust owns simulation and UI and
exchanges bounded JSON lines with an owned Python worker. Normal startup uses
real networking; only `--preview` bypasses the worker.
See [the main README](../readme.md) for a two-instance play test.

## Connections and hosting

Play discovers LAN hosts using IPv4 multicast. Refresh searches again. Creating
a lobby starts a Python server and connects its owner. Guests must mark Ready
before the host starts. Late joins are supported. Host departure or a broken
connection returns guests to the browser; there is no local simulation fallback.

Hosting automatically generates a versioned `jetgame://join?...` invitation.
Copy Join Link in the host lobby, paste it in the guest browser, and Connect.
On Windows Ctrl+C copies in the lobby and Ctrl+V pastes in the browser. Links
include a match ID and up to eight addresses; their label indicates Internet,
LAN, or this computer only. They are text links, not registered OS URL handlers.

Invitations try IPv6 before IPv4. Internet reachability requires firewall access
and possibly manual IPv4 port forwarding. Allow/forward **both TCP and UDP** on
the invitation port (default 29010). External and internal port numbers may differ.
There is no public directory, relay, automatic router configuration, or guaranteed
NAT traversal. IPv6 LAN discovery is not implemented; invitations can use IPv6.

```powershell
cargo run --offline -- --connect '[::1]:29010'
cargo run --offline -- --host-port 29010 --public-address example.net:40000
```

The example domain is a placeholder. Public-address overrides are optional;
normal hosts never need to construct a link themselves. Quote invitation strings
in shells because they contain `&`.

## Configuration and runtime

`networking.local.json` replaces `networking.json`. Rust reads `python` and
`agent_script`; remaining fields follow `jetgame_net/config.py`'s validated schema.
Set a listener address to an empty string to disable that IP family. Set
`lan_enabled` false for invitation-only hosts. `public_addresses` accepts complete
external addresses including ports.

Both debug and release use the private interpreter in `runtime/python`. There is
no fallback to a system Python installation. Prepare it once when developing:

```powershell
.\scripts\setup-runtime.ps1
```

Setup fetches the official Windows x64 embeddable archive pinned in
[`runtime/python-runtime.json`](../runtime/python-runtime.json), verifies its
SHA-256, and extracts it without installing Python or modifying the system.
The runtime binaries and download cache are ignored by Git. A fresh checkout
needs this development setup step; players receive the complete runtime.

```powershell
.\scripts\package.ps1
```

Packaging validates Python before and after copying with system Python absent
from PATH, then creates a ZIP and SHA-256 sidecar. It checks the ZIP contains
every runtime file. Packaging includes the private interpreter, standard-library ZIP, DLLs, restricted
`._pth`, and licenses along with the game, configuration, assets, and Python
sources. Distribute the generated ZIP and extract it completely before playing.
Do not move `python.exe` away from `python314.dll`, its standard library, or the
other runtime files. Windows packages target Windows 10/11 x64, as required by
[CPython 3.14](https://docs.python.org/3.14/using/windows.html).
No pip packages, global Python,
or startup downloads are required. Closing the game ends only its owned worker
and server processes. Missing/corrupt runtime files produce a clear error; hosting
errors preserve the host screen and its settings.

An explicit `--python ABSOLUTE_PATH`, `JETGAME_PYTHON`, or `python` configuration
can select a custom private runtime for development; packaged defaults always
point to the included runtime.

## Gameplay transport

The host advances fixed-rate physics (default 120 Hz) independently of rendering,
menus, and focus. Clients sample controls each tick and send chunks at 20 Hz,
including twelve redundant recent ticks by default. Python run-length encodes
held controls. The host deduplicates sequences, buffers jitter, consumes at most
one input per player per tick, and stops stale firing.

At 20 Hz, Python sends each guest a compressed, quantized state packet containing
its aircraft and consumed-input acknowledgement, plus rotating chunks of other
actors and projectiles. Health/life changes and incoming missiles have priority.
Default UDP payloads are capped at 1,200 bytes including authentication headers.
Removal tombstones and repeated events help recover loss. Under saturation,
lower-priority projectile visuals can be omitted; host collision remains decisive.

Clients predict their own flight and restore/replay unacknowledged inputs after
host updates. Remote jets interpolate with bounded extrapolation. Spawn/death
snaps reset visual history. Host state controls weapons, radar, health, and results.
`game.json`'s `sync` object configures rates, histories, chunks, and packet budgets.
Online limits are 32 human slots, 15 configured bots, and 256 projectiles.

TCP carries lobby state, preparation/start, participants, spawn/respawn requests,
action results, and round return. UDP carries inputs/state on the same numeric
port. Missing gameplay UDP produces a connection timeout, not an offline match.
Match epochs reject old-round traffic. Clients compare collision-level fingerprints;
no remote assets or executable content are loaded.

## Boundaries and extension

TCP frames and local IPC are capped at 65,536 bytes with validated messages,
timeouts, bounded queues, and operation IDs. Canceling a join ignores stale events.
Per-player UDP HMAC keys reject off-path spoofing. The TCP key exchange remains
plaintext: there is no authenticated encryption, match password, protection from
an on-path attacker, or protection from a malicious host. Lag compensation and
cross-network/high-latency validation remain unfinished.

`Lobby` owns participants/permissions; `server.py` and `client.py` own TCP sessions;
`discovery.py` and `links.py` own discovery/invitations; `realtime.py` owns UDP.
`agent.py` adapts these to the Rust bridge. `server_main.py` reuses the lobby server
without a UI, but full dedicated gameplay still needs a headless Rust authority
runner using `World::step_with_controls` and the existing command/event interface.
The standalone Python entry point alone is only a lobby host.

Tests: `.\runtime\python\python.exe -m unittest discover -s networking/tests -v` and
`cargo test --offline --test networking -- --ignored --nocapture`.
Multicast discovery tests need LAN socket permission, which loopback-only sandboxes
may block. Real Internet/CGNAT testing requires separate networks.
