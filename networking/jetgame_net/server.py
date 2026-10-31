"""Headless socket adapter around Lobby; shared by embedded and standalone hosts."""

import asyncio
from collections import deque
import contextlib
from dataclasses import replace
import logging
import secrets
import socket
import time
from .config import NetworkSettings
from .discovery import advertise
from .lobby import Lobby
from .links import invitation
from .realtime import GameRelay, integer
from .protocol import ProtocolError, receive, send

LOG = logging.getLogger(__name__)


class MatchServer:
    def __init__(self, name="DOGFIGHT", max_players=8, map_name="WORLD 01", mode="TEAM DOGFIGHT", settings=None):
        self.settings = settings or NetworkSettings()
        if type(max_players) is not int or not 2 <= max_players <= self.settings.max_players:
            raise ProtocolError("Invalid player limit")
        self.lobby = Lobby(name, max_players, map_name, mode)
        self.servers = []
        self.clients = {}
        self.tasks = set()
        self.discovery = None
        self.discovery_warning = ""
        self.port = 0
        self.local_endpoint = ""
        self.slots = {}
        self.game = None
        self.preparing = 0
        self.epoch_counter = secrets.randbits(32) or 1
        self.on_inputs = None

    @property
    def admin_token(self):
        return self.lobby.admin_token

    async def start(self):
        errors = []
        for family, host in ((socket.AF_INET6, self.settings.listen_ipv6), (socket.AF_INET, self.settings.listen_ipv4)):
            if not host:
                continue
            sock = None
            try:
                sock = socket.socket(family, socket.SOCK_STREAM)
                if family == socket.AF_INET6:
                    sock.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
                if hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
                    sock.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
                sock.bind((host, self.port or self.settings.host_port))
                sock.listen(64)
                sock.setblocking(False)
                server = await asyncio.start_server(self.handle, sock=sock, limit=65540)
                self.servers.append(server)
                self.port = sock.getsockname()[1]
                if not self.local_endpoint:
                    address = "::1" if host == "::" else "127.0.0.1" if host == "0.0.0.0" else host
                    self.local_endpoint = f"[{address}]:{self.port}" if family == socket.AF_INET6 else f"{address}:{self.port}"
            except OSError as error:
                if sock:
                    sock.close()
                errors.append(error)
        if not self.servers:
            raise OSError(f"Cannot listen for matches: {errors}")
        if self.settings.lan_enabled and any(s.sockets[0].family == socket.AF_INET for s in self.servers):
            try:
                self.discovery = await advertise(self.settings, self.listing)
            except OSError as error:
                self.discovery_warning = f"LAN discovery unavailable: {error}"
                LOG.warning(self.discovery_warning)
        return self.port

    def listing(self):
        return {"id": self.lobby.id, "name": self.lobby.name, "port": self.port,
                "players": len(self.lobby.players), "max_players": self.lobby.max_players,
                "in_progress": self.game is not None}

    def participants(self):
        return [{"id": key, "slot": self.slots[key], "name": player["name"]} for key, player in self.lobby.players.items()]

    async def send_all(self, kind, payload):
        async def one(writer):
            try:
                await send(writer, kind, payload)
            except (OSError, TimeoutError):
                writer.close()
        await asyncio.gather(*(one(writer) for writer in list(self.clients.values())))

    async def participant_update(self):
        if self.game:
            await self.send_all("participants", {"epoch": self.game.setup["epoch"], "players": self.participants()})

    async def prepare(self, player):
        writer = self.clients[player]
        if not self.lobby.players[player]["host"]:
            await send(writer, "error", {"code": "host_required", "message": "Only the host may start a match"})
        elif self.game or self.preparing:
            await send(writer, "error", {"message": "A match is already starting or running"})
        elif not all(p["ready"] for p in self.lobby.players.values()):
            await send(writer, "error", {"message": "All players must be ready"})
        else:
            self.epoch_counter = (self.epoch_counter + 1) % 2**32 or 1
            self.preparing = self.epoch_counter
            await send(writer, "prepare", {"epoch": self.preparing, "players": self.participants(),
                                          "capacity": self.lobby.max_players, "free_for_all": self.lobby.mode == "FREE FOR ALL"})

    async def begin(self, setup):
        if not isinstance(setup, dict) or setup.get("epoch") != self.preparing or not self.preparing or setup.get("capacity") != self.lobby.max_players:
            raise ProtocolError("Match preparation is stale")
        sync, rules = setup.get("sync"), setup.get("rules")
        if not isinstance(sync, dict) or not isinstance(rules, dict) or not integer(rules.get("physics_hz"), 30, 240) or not integer(rules.get("bot_count"), 0, 15) or not integer(rules.get("max_projectiles"), 1, 256):
            raise ProtocolError("Invalid gameplay settings")
        for key, low, high in (("max_packet_bytes", 512, 1200), ("actor_chunk", 1, 8), ("projectile_chunk", 1, 32), ("input_hz", 5, 30), ("snapshot_hz", 5, 30)):
            if not integer(sync.get(key), low, high):
                raise ProtocolError("Invalid synchronization settings")
        relay = GameRelay(self, setup, self.on_inputs)
        try:
            await relay.start()
        except OSError:
            self.preparing = 0
            raise ProtocolError("Gameplay UDP port is unavailable") from None
        self.game = relay
        self.preparing = 0
        for player in list(self.clients):
            await self.start_client(player)

    async def start_client(self, player):
        if self.game and player in self.clients:
            await send(self.clients[player], "game_started", {"setup": self.game.setup, "players": self.participants(),
                        "udp_key": self.game.credential(player)})

    async def game_action(self, player, payload):
        if not self.game:
            return
        if set(payload) != {"epoch", "action", "action_id", "team"} or not integer(payload["epoch"], 1, 2**32 - 1) or payload["action"] not in ("spawn", "respawn", "end") or not integer(payload["action_id"], 1, 2**32 - 1) or not integer(payload["team"], 0, 1):
            raise ProtocolError("Invalid gameplay action")
        if payload["epoch"] != self.game.setup["epoch"]:
            return
        if payload["action"] == "end" and not self.lobby.players[player]["host"]:
            await send(self.clients[player], "error", {"message": "Only the host may end the match"})
            return
        host = next((key for key, p in self.lobby.players.items() if p["host"]), None)
        if host in self.clients:
            await send(self.clients[host], "action", {"epoch": self.game.setup["epoch"], "player": player, **payload})

    async def action_result(self, epoch, player, action_id, accepted, reason):
        if self.game and epoch == self.game.setup["epoch"] and player in self.clients:
            await send(self.clients[player], "action_result", {"epoch": self.game.setup["epoch"], "action_id": action_id,
                        "accepted": accepted, "reason": str(reason)[:160]})

    async def return_lobby(self):
        if self.game:
            self.game.close()
            self.game = None
        self.preparing = 0
        for player in self.lobby.players.values():
            player["ready"] = player["host"]
        await self.send_all("returned_lobby", {})
        await self.broadcast()

    async def invitation(self):
        families = {server.sockets[0].family for server in self.servers}
        if not families:
            raise ProtocolError("Host must be listening before generating a join link")
        settings = replace(self.settings,
                           listen_ipv6=self.settings.listen_ipv6 if socket.AF_INET6 in families else "",
                           listen_ipv4=self.settings.listen_ipv4 if socket.AF_INET in families else "")
        return await invitation(settings, self.port, self.lobby.id)

    async def broadcast(self):
        payload = {"players": self.lobby.roster()}
        async def one(writer):
            try:
                await send(writer, "lobby", payload)
            except (ConnectionError, TimeoutError):
                writer.close()
        await asyncio.gather(*(one(w) for w in list(self.clients.values())))

    async def handle(self, reader, writer):
        task = asyncio.current_task()
        if len(self.tasks) >= self.settings.max_connections:
            writer.close()
            return
        self.tasks.add(task)
        player_id = None
        rate = deque()
        try:
            message = await receive(reader, self.settings.handshake_timeout)
            if message["kind"] != "join":
                raise ProtocolError("Join handshake required")
            player_id = self.lobby.join(message["payload"])
            self.slots[player_id] = next(slot for slot in range(self.lobby.max_players) if slot not in self.slots.values())
            self.clients[player_id] = writer
            await send(writer, "welcome", self.lobby.welcome(player_id), message["request_id"])
            await self.broadcast()
            if self.game:
                await self.start_client(player_id)
                await self.participant_update()
            while True:
                message = await receive(reader, self.settings.idle_timeout)
                now = time.monotonic()
                while rate and rate[0] < now - 1:
                    rate.popleft()
                if len(rate) >= 30:
                    raise ProtocolError("Too many requests")
                rate.append(now)
                kind = message["kind"]
                if kind == "leave":
                    break
                if kind == "start_match":
                    await self.prepare(player_id)
                    continue
                if kind == "game_action":
                    await self.game_action(player_id, message["payload"])
                    continue
                if kind == "ready" and (self.game or self.preparing):
                    await send(writer, "error", {"message": "Ready cannot change during a match"})
                    continue
                reply = self.lobby.command(player_id, kind, message["payload"])
                if reply:
                    await send(writer, reply[0], reply[1], message["request_id"])
                else:
                    await self.broadcast()
        except ProtocolError as error:
            with contextlib.suppress(ConnectionError, TimeoutError):
                await send(writer, "error", {"message": str(error)})
        except (asyncio.IncompleteReadError, ConnectionError, TimeoutError):
            pass
        finally:
            was_host = self.lobby.players.get(player_id, {}).get("host", False)
            self.lobby.players.pop(player_id, None)
            self.slots.pop(player_id, None)
            if self.game:
                self.game.peers.pop(player_id, None)
                if was_host:
                    self.game.close()
                    self.game = None
                    for other in self.clients.values():
                        if other is not writer:
                            other.close()
            self.clients.pop(player_id, None)
            writer.close()
            with contextlib.suppress(ConnectionError, TimeoutError):
                await asyncio.wait_for(writer.wait_closed(), 1)
            await self.broadcast()
            await self.participant_update()
            self.tasks.discard(task)

    async def close(self):
        if self.game:
            self.game.close()
            self.game = None
        if self.discovery:
            self.discovery.close()
            self.discovery = None
        for server in self.servers:
            server.close()
        for writer in list(self.clients.values()):
            writer.close()
        for task in list(self.tasks):
            task.cancel()
        if self.tasks:
            await asyncio.gather(*list(self.tasks), return_exceptions=True)
        # Newer Python versions wait for active transports here, so close them first.
        for server in self.servers:
            await server.wait_closed()
        self.servers.clear()
