"""Central directory skeleton. Deploy behind TLS/auth/reachability validation later."""

import asyncio
import contextlib
import secrets
import time
from .protocol import ProtocolError, clean_text, receive, send

LEASE_SECONDS = 20
MAX_MATCHES = 512


class Directory:
    def __init__(self):
        self.matches = {}
        self.server = None

    def expire(self):
        now = time.monotonic()
        self.matches = {key: entry for key, entry in self.matches.items() if entry["expires"] > now}

    async def start(self, host="127.0.0.1", port=29000):
        self.server = await asyncio.start_server(self.handle, host, port, limit=65540)
        return self.server.sockets[0].getsockname()[1]

    async def close(self):
        if self.server:
            self.server.close()
            await self.server.wait_closed()

    async def handle(self, reader, writer):
        try:
            message = await receive(reader, 3)
            self.expire()
            kind, data, req = message["kind"], message["payload"], message["request_id"]
            peer = writer.get_extra_info("peername")[0]
            if kind == "discover":
                offset = data.get("offset", 0)
                if type(offset) is not int or not 0 <= offset <= MAX_MATCHES:
                    raise ProtocolError("invalid page offset")
                all_entries = list(self.matches.values())
                entries = [entry["listing"] for entry in all_entries[offset:offset + 100]]
                next_offset = offset + 100 if offset + 100 < len(all_entries) else None
                await send(writer, "servers", {"servers": entries, "next_offset": next_offset}, req)
            elif kind == "register":
                port = data.get("port")
                maximum = data.get("max_players", 8)
                if type(port) is not int or not 1 <= port <= 65535 or type(maximum) is not int or not 2 <= maximum <= 32:
                    raise ProtocolError("invalid advertised port/player limit")
                if len(self.matches) >= MAX_MATCHES:
                    raise ProtocolError("directory is full")
                match_id, token = secrets.token_hex(12), secrets.token_hex(24)
                # Use the observed address, never a caller-supplied arbitrary host.
                endpoint = f"[{peer}]:{port}" if ":" in peer else f"{peer}:{port}"
                listing = {"id": match_id, "name": clean_text(data.get("name"), "DOGFIGHT"),
                           "endpoint": endpoint, "players": 0, "max_players": maximum,
                           "in_progress": False, "reachability": "unverified"}
                self.matches[match_id] = {"token": token, "owner": peer, "listing": listing,
                                          "expires": time.monotonic() + LEASE_SECONDS}
                await send(writer, "registered", {"id": match_id, "lease_token": token,
                                                   "heartbeat_seconds": 5}, req)
            elif kind in ("heartbeat", "unregister"):
                entry = self.matches.get(data.get("id"))
                token = data.get("lease_token")
                if not entry or not isinstance(token, str) or not secrets.compare_digest(entry["token"], token) or entry["owner"] != peer:
                    raise ProtocolError("invalid directory lease")
                if kind == "unregister":
                    del self.matches[data["id"]]
                else:
                    count = data.get("players", 0)
                    if type(count) is not int or not 0 <= count <= entry["listing"]["max_players"]:
                        raise ProtocolError("invalid player count")
                    entry["listing"]["players"] = count
                    entry["expires"] = time.monotonic() + LEASE_SECONDS
                await send(writer, "ok", {}, req)
            else:
                raise ProtocolError("unsupported directory request")
        except (ProtocolError, ValueError) as error:
            try:
                await send(writer, "error", {"message": str(error)})
            except (ConnectionError, TimeoutError):
                pass
        except (asyncio.IncompleteReadError, ConnectionError, TimeoutError):
            pass
        finally:
            writer.close()
            with contextlib.suppress(ConnectionError):
                await writer.wait_closed()
