"""Headless socket adapter around Lobby; shared by embedded and standalone hosts."""

import asyncio
from collections import deque
import contextlib
from dataclasses import replace
import logging
import socket
import time
from .config import NetworkSettings
from .discovery import advertise
from .lobby import Lobby
from .links import invitation
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
                "players": len(self.lobby.players), "max_players": self.lobby.max_players}

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
            self.clients[player_id] = writer
            await send(writer, "welcome", self.lobby.welcome(player_id), message["request_id"])
            await self.broadcast()
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
            self.lobby.players.pop(player_id, None)
            self.clients.pop(player_id, None)
            writer.close()
            with contextlib.suppress(ConnectionError, TimeoutError):
                await asyncio.wait_for(writer.wait_closed(), 1)
            await self.broadcast()
            self.tasks.discard(task)

    async def close(self):
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
