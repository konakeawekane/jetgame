"""IPv6-first connection policy and lobby client, independent of Rust or any UI."""

import asyncio
import contextlib
import socket
import time
from .links import endpoint, parse_link, split_endpoint
from .protocol import ProtocolError, receive, send
from .realtime import ClientDatagrams


async def resolve_candidates(addresses, settings):
    loop = asyncio.get_running_loop()
    candidates = []
    for address in addresses:
        host, port = split_endpoint(address)
        try:
            records = await asyncio.wait_for(loop.getaddrinfo(host, port, type=socket.SOCK_STREAM), settings.connect_timeout)
        except (OSError, TimeoutError):
            continue
        for family, _, _, _, sock in records:
            item = (family, sock[0], sock[1])
            if family in (socket.AF_INET, socket.AF_INET6) and item not in candidates:
                candidates.append(item)
    return sorted(candidates[:32], key=lambda a: a[0] != socket.AF_INET6)


class Connection:
    def __init__(self, reader, writer, info, address, settings):
        self.reader, self.writer = reader, writer
        self.info, self.endpoint, self.settings = info, address, settings
        self.write_lock = asyncio.Lock()
        self.game = None

    @classmethod
    async def join(cls, invitation, settings, admin_token=""):
        addresses, expected_id = parse_link(invitation)
        candidates = await resolve_candidates(addresses, settings)
        errors = []
        for family, host, port in candidates:
            writer = None
            try:
                reader, writer = await asyncio.wait_for(asyncio.open_connection(host, port, family=family, limit=65540), settings.connect_timeout)
                await send(writer, "join", {"name": settings.player_name, "admin_token": admin_token}, 1)
                reply = await receive(reader, settings.handshake_timeout)
                if reply["kind"] != "welcome":
                    raise ProtocolError(str(reply["payload"].get("message", "Join rejected"))[:160])
                info = reply["payload"]
                validate_welcome(info)
                if expected_id and info["id"] != expected_id:
                    raise ProtocolError("Invitation belongs to a different match")
                return cls(reader, writer, info, endpoint(host, port), settings)
            except (OSError, TimeoutError, ProtocolError, asyncio.IncompleteReadError) as error:
                errors.append(str(error))
                if writer:
                    writer.close()
            except BaseException:
                if writer:
                    writer.close()
                raise
        detail = errors[-1] if errors else "No address resolved"
        raise ProtocolError(f"Could not join: {detail}. Check IPv6 firewall access or IPv4 port forwarding.")

    async def send(self, kind, payload=None):
        async with self.write_lock:
            await send(self.writer, kind, payload)

    async def listen(self, emit):
        async def heartbeat():
            while True:
                await asyncio.sleep(self.settings.ping_seconds)
                if self.game and time.monotonic() - self.game.last_state > self.settings.idle_timeout:
                    raise ProtocolError("Gameplay UDP timed out; allow or forward UDP on the same port as TCP")
                await self.send("ping")
        ping = asyncio.create_task(heartbeat())
        try:
            while True:
                message = await receive(self.reader, self.settings.idle_timeout)
                kind, payload = message["kind"], message["payload"]
                if kind == "lobby":
                    validate_roster(payload.get("players"))
                    emit("lobby", players=payload["players"])
                elif kind == "error":
                    emit("error", message=str(payload.get("message", "Server error"))[:160])
                elif kind == "game_started":
                    if self.game:
                        self.game.close()
                        await self.game.wait_closed()
                    if not isinstance(payload.get("setup"), dict) or not isinstance(payload.get("players"), list):
                        raise ProtocolError("Invalid match start")
                    if not self.info["host"]:
                        self.game = await ClientDatagrams.start(self.writer, payload["setup"], self.info["player_id"],
                                                               payload.get("udp_key"), emit)
                    emit("game_started", setup=payload["setup"], players=payload["players"])
                elif kind in ("prepare", "participants", "action", "action_result"):
                    emit(kind, **payload)
                elif kind == "returned_lobby":
                    if self.game:
                        self.game.close()
                        await self.game.wait_closed()
                        self.game = None
                    emit("returned_lobby")
                elif kind != "pong":
                    raise ProtocolError("Unsupported server message")
                if ping.done():
                    ping.result()
        finally:
            ping.cancel()
            with contextlib.suppress(asyncio.CancelledError, OSError, TimeoutError):
                await ping

    async def close(self):
        if self.game:
            self.game.close()
            await self.game.wait_closed()
            self.game = None
        self.writer.close()
        with contextlib.suppress(OSError, TimeoutError):
            await asyncio.wait_for(self.writer.wait_closed(), 1)


def validate_roster(players):
    if not isinstance(players, list) or len(players) > 32:
        raise ProtocolError("Invalid roster")
    ids = set()
    for player in players:
        if not isinstance(player, dict) or type(player.get("id")) is not int or not 1 <= player["id"] < 2**64 or player["id"] in ids:
            raise ProtocolError("Invalid player ID")
        ids.add(player["id"])
        if not isinstance(player.get("name"), str) or not 1 <= len(player["name"]) <= 24 or not player["name"].isascii() or not player["name"].isprintable() or type(player.get("ready")) is not bool or type(player.get("host")) is not bool:
            raise ProtocolError("Invalid player")


def validate_welcome(info):
    validate_roster(info.get("players"))
    if type(info.get("player_id")) is not int or not any(p["id"] == info["player_id"] for p in info["players"]) or type(info.get("host")) is not bool or type(info.get("max_players")) is not int or not 2 <= info["max_players"] <= 32 or len(info["players"]) > info["max_players"]:
        raise ProtocolError("Invalid session")
    for key in ("name", "map", "mode", "id"):
        value = info.get(key)
        if not isinstance(value, str) or not 1 <= len(value) <= 32 or not value.isascii() or not value.isprintable():
            raise ProtocolError("Invalid session description")
    if not isinstance(info.get("capabilities"), list) or "lobby" not in info["capabilities"]:
        raise ProtocolError("Unsupported server capabilities")
