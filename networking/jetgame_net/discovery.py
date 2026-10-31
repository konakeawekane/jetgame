"""Small link-local IPv4 multicast discovery. Listings are always untrusted."""

import asyncio
import json
import secrets
import socket
import time
from .links import endpoint, local_addresses

MAX_DATAGRAM = 1200
MAGIC = "jetgame-lan-v1"


def packet(data):
    body = json.dumps(data, separators=(",", ":"), allow_nan=False).encode("utf-8")
    if len(body) > MAX_DATAGRAM:
        raise ValueError("Discovery packet exceeds size limit")
    return body


def decode(data):
    if len(data) > MAX_DATAGRAM:
        raise ValueError("Oversized datagram")
    message = json.loads(data)
    if not isinstance(message, dict) or message.get("protocol") != MAGIC or not isinstance(message.get("nonce"), str) or len(message["nonce"]) != 24:
        raise ValueError("Invalid discovery packet")
    return message


class Responder(asyncio.DatagramProtocol):
    def __init__(self, listing):
        self.listing = listing
        self.last_reply = {}

    def connection_made(self, transport):
        self.transport = transport

    def datagram_received(self, data, addr):
        try:
            request = decode(data)
            if request.get("kind") != "query":
                return
            now = time.monotonic()
            if now - self.last_reply.get(addr[0], 0) < 0.2:
                return
            if len(self.last_reply) >= 256:
                self.last_reply.clear()
            self.last_reply[addr[0]] = now
            self.transport.sendto(packet({"protocol": MAGIC, "kind": "listing", "nonce": request["nonce"], **self.listing()}), addr)
        except (ValueError, UnicodeError, OSError):
            pass


async def advertise(settings, listing):
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_LOOP, 1)
        sock.bind(("", settings.discovery_port))
        joined = 0
        for interface in await interfaces(settings):
            membership = socket.inet_aton(settings.discovery_group) + socket.inet_aton(interface)
            try:
                sock.setsockopt(socket.IPPROTO_IP, socket.IP_ADD_MEMBERSHIP, membership)
                joined += 1
            except OSError:
                pass
        if not joined:
            raise OSError("No multicast-capable LAN interface")
        sock.setblocking(False)
        transport, _ = await asyncio.get_running_loop().create_datagram_endpoint(lambda: Responder(listing), sock=sock)
        return transport
    except BaseException:
        sock.close()
        raise


async def discover(settings):
    if not settings.lan_enabled:
        return []
    nonce = secrets.token_hex(12)
    results = {}

    class Collector(asyncio.DatagramProtocol):
        def datagram_received(self, data, addr):
            try:
                message = decode(data)
                if message.get("kind") != "listing" or message["nonce"] != nonce:
                    return
                port, count, maximum = message["port"], message["players"], message["max_players"]
                name, match_id = message["name"], message["id"]
                if type(port) is not int or not 1 <= port <= 65535 or type(count) is not int or type(maximum) is not int or not 0 <= count <= maximum <= 32:
                    return
                if not isinstance(name, str) or not 1 <= len(name) <= 32 or not name.isascii() or not name.isprintable() or not isinstance(match_id, str) or len(match_id) != 32:
                    return
                if len(results) < settings.max_listings:
                    key = (match_id, addr[0], port)
                    results[key] = {"id": match_id, "name": name, "endpoint": endpoint(addr[0], port), "players": count,
                                    "max_players": maximum, "in_progress": message.get("in_progress") is True, "reachability": "LAN"}
            except (ValueError, KeyError, TypeError, UnicodeError):
                pass

    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    transport = None
    try:
        targets = await interfaces(settings)
        sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, 1)
        sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_LOOP, 1)
        sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_IF, socket.inet_aton(settings.discovery_interface))
        sock.bind((settings.discovery_interface, 0))
        sock.setblocking(False)
        transport, _ = await asyncio.get_running_loop().create_datagram_endpoint(Collector, sock=sock)
        query = packet({"protocol": MAGIC, "kind": "query", "nonce": nonce})
        # Retry once because UDP discovery is best effort.
        for _ in range(2):
            for interface in targets:
                try:
                    sock.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_IF, socket.inet_aton(interface))
                    transport.sendto(query, (settings.discovery_group, settings.discovery_port))
                except OSError:
                    pass
            await asyncio.sleep(settings.discovery_seconds / 2)
        return sorted(results.values(), key=lambda s: (s["name"], s["endpoint"]))
    finally:
        if transport:
            transport.close()
        else:
            sock.close()


async def interfaces(settings):
    if settings.discovery_interface != "0.0.0.0":
        return [settings.discovery_interface]
    addresses = [address for address in await local_addresses() if ":" not in address]
    return list(dict.fromkeys(["0.0.0.0", *addresses]))
