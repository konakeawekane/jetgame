"""Lobby server. Authoritative simulation/real-time transport are intentionally not selected."""

import asyncio
import contextlib
import logging
import secrets
from .protocol import ProtocolError, clean_text, receive, request, send

LOG = logging.getLogger(__name__)


class MatchServer:
    def __init__(self, name="DOGFIGHT", max_players=8, map_name="WORLD 01", mode="TEAM DOGFIGHT", directory=None, public_port=None):
        self.name = clean_text(name, "DOGFIGHT")
        self.max_players = max(2, min(32, max_players))
        self.map_name, self.mode = clean_text(map_name, "WORLD 01"), clean_text(mode, "TEAM DOGFIGHT")
        self.directory, self.public_port = directory, public_port
        self.admin_token = secrets.token_hex(24)
        self.players = {}
        self.next_id = 1
        self.server = None
        self.registration = None
        self.advertisement = None
        self.tasks = set()

    async def start(self, host="127.0.0.1", port=0):
        self.server = await asyncio.start_server(self.handle, host, port, limit=65540)
        self.port = self.server.sockets[0].getsockname()[1]
        if self.directory:
            self.advertisement = asyncio.create_task(self.advertise())
        return self.port

    def roster(self):
        return [{"id": key, "name": p["name"], "ready": p["ready"], "host": p["host"]} for key, p in self.players.items()]

    async def broadcast(self):
        async def one(player):
            try:
                await send(player["writer"], "lobby", {"players": self.roster()})
            except (ConnectionError, TimeoutError):
                player["writer"].close()
        await asyncio.gather(*(one(p) for p in list(self.players.values())))

    async def handle(self, reader, writer):
        task = asyncio.current_task()
        self.tasks.add(task)
        player_id = None
        try:
            message = await receive(reader, 3)
            if message["kind"] != "join":
                raise ProtocolError("join handshake required")
            if len(self.players) >= self.max_players:
                raise ProtocolError("match is full")
            token = message["payload"].get("admin_token", "")
            host = isinstance(token, str) and secrets.compare_digest(token, self.admin_token)
            player_id, self.next_id = self.next_id, self.next_id + 1
            self.players[player_id] = {"writer": writer, "name": clean_text(message["payload"].get("name"), "PILOT", 24),
                                       "host": host, "ready": host}
            await send(writer, "welcome", {"name": self.name, "player_id": player_id, "host": host,
                       "max_players": self.max_players, "map": self.map_name, "mode": self.mode,
                       "capabilities": ["lobby"], "players": self.roster()}, message["request_id"])
            await self.broadcast()
            while True:
                message = await receive(reader, 10)
                kind, payload, req = message["kind"], message["payload"], message["request_id"]
                if kind == "ping":
                    await send(writer, "pong", {}, req)
                elif kind == "ready":
                    if type(payload.get("ready")) is not bool:
                        raise ProtocolError("ready must be boolean")
                    self.players[player_id]["ready"] = payload["ready"]
                    await self.broadcast()
                elif kind == "start_match":
                    reason = "Online gameplay synchronization is not implemented" if host else "Only the host may start a match"
                    await send(writer, "error", {"code": "gameplay_not_implemented" if host else "host_required", "message": reason}, req)
                elif kind == "leave":
                    break
                else:
                    await send(writer, "error", {"message": "Unsupported lobby request"}, req)
        except ProtocolError as error:
            with contextlib.suppress(ConnectionError, TimeoutError):
                await send(writer, "error", {"message": str(error)})
        except (asyncio.IncompleteReadError, ConnectionError, TimeoutError):
            pass
        finally:
            self.players.pop(player_id, None)
            writer.close()
            with contextlib.suppress(ConnectionError):
                await writer.wait_closed()
            await self.broadcast()
            self.tasks.discard(task)

    async def advertise(self):
        while True:
            try:
                if not self.registration:
                    self.registration = await request(self.directory, "register", {"port": self.public_port or self.port,
                        "name": self.name, "max_players": self.max_players})
                    LOG.info("Registered directory listing %s", self.registration["id"])
                await request(self.directory, "heartbeat", {**self.registration, "players": len(self.players)})
            except (OSError, TimeoutError, ProtocolError, ValueError) as error:
                LOG.warning("Directory advertisement: %s", error)
                self.registration = None
            await asyncio.sleep(5)

    async def close(self):
        if self.advertisement:
            self.advertisement.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await self.advertisement
        if self.directory and self.registration:
            with contextlib.suppress(OSError, TimeoutError, ProtocolError):
                await request(self.directory, "unregister", self.registration)
        if self.server:
            self.server.close()
            await self.server.wait_closed()
        for player in list(self.players.values()):
            player["writer"].close()
        if self.tasks:
            await asyncio.gather(*list(self.tasks), return_exceptions=True)
