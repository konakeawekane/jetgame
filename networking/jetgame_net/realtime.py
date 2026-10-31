"""Bounded authenticated UDP, RLE input chunks, and budgeted state trickling.

The per-session key is negotiated over the existing control connection. This
filters off-path spoofing; plaintext TCP is not protection from an on-path attacker.
"""

import asyncio
import hashlib
import hmac
import json
import secrets
import socket
import struct
import time
import zlib
from dataclasses import dataclass, field
from .protocol import ProtocolError

MAX_DATAGRAM = 1200
HEADER = struct.Struct("!4sIQ")
RUN = struct.Struct("!IBhhhHB")
TAG = 16


def integer(value, low, high):
    return type(value) is int and low <= value <= high


def signed_packet(kind, epoch, player, body, key):
    message = HEADER.pack(kind, epoch, player) + body
    return message + hmac.digest(key, message, hashlib.sha256)[:TAG]


def open_packet(data, kind, epoch, player, key):
    if not HEADER.size + TAG <= len(data) <= MAX_DATAGRAM:
        raise ProtocolError("Invalid datagram size")
    if HEADER.unpack_from(data) != (kind, epoch, player) or not hmac.compare_digest(data[-TAG:], hmac.digest(key, data[:-TAG], hashlib.sha256)[:TAG]):
        raise ProtocolError("Invalid datagram authentication")
    return data[HEADER.size:-TAG]


def validate_frame(frame):
    if not isinstance(frame, list) or len(frame) != 6 or any(not integer(v, -1000, 1000) for v in frame[:3]) or not integer(frame[3], 0, 1000) or any(not integer(v, 0, 1) for v in frame[4:]):
        raise ProtocolError("Invalid input sample")


def pack_inputs(first, frames):
    if not integer(first, 1, 2**32 - 65) or not isinstance(frames, list) or not 1 <= len(frames) <= 64:
        raise ProtocolError("Invalid input chunk")
    for frame in frames:
        validate_frame(frame)
    result = bytearray()
    index = 0
    while index < len(frames):
        frame = frames[index]
        validate_frame(frame)
        count = 1
        while index + count < len(frames) and frames[index + count] == frame:
            count += 1
        result.extend(RUN.pack(first + index, count, *frame[:4], frame[4] | frame[5] << 1))
        index += count
    return bytes(result)


def unpack_inputs(body):
    if not body:
        return 0, []  # Authenticated endpoint registration.
    if len(body) % RUN.size or len(body) > 64 * RUN.size:
        raise ProtocolError("Invalid input chunk")
    first = None
    frames = []
    for offset in range(0, len(body), RUN.size):
        seq, count, pitch, yaw, roll, throttle, flags = RUN.unpack_from(body, offset)
        frame = [pitch, yaw, roll, throttle, flags & 1, flags >> 1]
        validate_frame(frame)
        if first is None:
            first = seq
        if not 1 <= count <= 64 or seq != first + len(frames) or len(frames) + count > 64 or not integer(seq + count, 2, 2**32 - 1):
            raise ProtocolError("Invalid input sequence")
        frames.extend([frame] * count)
    return first, frames


def validate_rows(rows, width, maximum):
    if not isinstance(rows, list) or len(rows) > maximum or any(not isinstance(row, list) or len(row) != width or any(not integer(v, -(2**32), 2**53) for v in row) for row in rows):
        raise ProtocolError("Invalid state rows")


def validate_state(packet):
    if not isinstance(packet, dict) or set(packet) != {"t", "e", "f", "k", "a", "p", "x", "r"} or not integer(packet["t"], 0, 2**32 - 1) or not integer(packet["e"], 0, 2**32 - 1) or type(packet["f"]) is not bool or not integer(packet["k"], 0, 2**32 - 1):
        raise ProtocolError("Invalid state packet")
    validate_rows(packet["a"], 32, 47)
    validate_rows(packet["p"], 12, 32)
    validate_rows(packet["x"], 6, 8)
    if not isinstance(packet["r"], list) or len(packet["r"]) > 64 or any(not integer(v, 1, 2**32 - 1) for v in packet["r"]):
        raise ProtocolError("Invalid removed entities")


def compress_state(packet):
    return zlib.compress(json.dumps(packet, separators=(",", ":"), allow_nan=False).encode(), 1)


def decompress_state(body):
    decoder = zlib.decompressobj()
    data = decoder.decompress(body, 32769)
    if len(data) > 32768 or decoder.unconsumed_tail or decoder.unused_data or not decoder.eof:
        raise ProtocolError("Invalid compressed state")
    packet = json.loads(data)
    validate_state(packet)
    return packet


class ClientDatagrams(asyncio.DatagramProtocol):
    def __init__(self, epoch, player, key, emit):
        self.epoch, self.player, self.key, self.emit = epoch, player, key, emit
        self.transport = None
        self.last_tick = -1
        self.last_state = time.monotonic()
        self.closed = asyncio.Event()

    def connection_lost(self, exc):
        self.closed.set()

    async def wait_closed(self):
        await asyncio.wait_for(self.closed.wait(), 1)

    @classmethod
    async def start(cls, writer, setup, player, key, emit):
        if not integer(setup.get("epoch"), 1, 2**32 - 1) or not isinstance(key, str) or len(key) != 64:
            raise ProtocolError("Invalid game channel")
        try:
            key = bytes.fromhex(key)
        except ValueError:
            raise ProtocolError("Invalid game credential") from None
        peer = writer.get_extra_info("peername")
        # Share the externally visible TCP invitation port, not the internal
        # server port, so forwarding both protocols to a different port works.
        port = peer[1]
        family = writer.get_extra_info("socket").family
        host = peer[0]
        if family == socket.AF_INET6 and len(peer) > 3 and peer[3] and "%" not in host:
            host += f"%{peer[3]}"
        protocol = cls(setup["epoch"], player, key, emit)
        # Let asyncio establish the connected endpoint itself. Passing an already
        # connected socket loses its peer address on Windows' Proactor transport.
        transport, _ = await asyncio.get_running_loop().create_datagram_endpoint(
            lambda: protocol, remote_addr=(host, port), family=family)
        protocol.transport = transport
        transport.sendto(signed_packet(b"JG2I", protocol.epoch, player, b"", key))
        return protocol

    def datagram_received(self, data, addr):
        try:
            body = open_packet(data, b"JG2S", self.epoch, self.player, self.key)
            packet = decompress_state(body)
            if packet["t"] <= self.last_tick:
                return
            self.last_tick = packet["t"]
            self.last_state = time.monotonic()
            self.emit("state", epoch=self.epoch, packet=packet)
        except (ValueError, UnicodeError, zlib.error, RecursionError):
            pass

    def send_inputs(self, first, frames):
        body = pack_inputs(first, frames)
        if self.transport and not self.transport.is_closing():
            self.transport.sendto(signed_packet(b"JG2I", self.epoch, self.player, body, self.key))

    def close(self):
        if self.transport:
            # Old-round UDP packets should be discarded rather than drained.
            self.transport.abort()
            self.transport = None


@dataclass
class PeerStream:
    key: bytes = field(default_factory=lambda: secrets.token_bytes(32))
    address: tuple | None = None
    transport: object = None
    cursor: int = 0
    removed_cursor: int = 0
    projectile_counts: dict = field(default_factory=dict)
    explosion_counts: dict = field(default_factory=dict)
    actor_status: dict = field(default_factory=dict)
    rate_time: float = 0
    rate_count: int = 0


class ServerDatagrams(asyncio.DatagramProtocol):
    def __init__(self, relay):
        self.relay = relay

    def connection_made(self, transport):
        self.transport = transport

    def datagram_received(self, data, addr):
        try:
            if not HEADER.size + TAG <= len(data) <= MAX_DATAGRAM:
                return
            _, epoch, player = HEADER.unpack_from(data)
            peer = self.relay.peers.get(player)
            if not peer or epoch != self.relay.setup["epoch"]:
                return
            body = open_packet(data, b"JG2I", epoch, player, peer.key)
            now = time.monotonic()
            if now - peer.rate_time >= 1:
                peer.rate_time, peer.rate_count = now, 0
            peer.rate_count += 1
            if peer.rate_count > 60:
                return
            first, frames = unpack_inputs(body)
            peer.address, peer.transport = addr, self.transport
            if frames and self.relay.on_inputs:
                self.relay.on_inputs(epoch=epoch, player=player, first=first, frames=frames)
        except (ValueError, struct.error):
            pass


class GameRelay:
    def __init__(self, server, setup, on_inputs):
        self.server, self.setup, self.on_inputs = server, setup, on_inputs
        self.transports = []
        self.peers = {}
        self.last_tick = -1
        self.active_projectiles = set()
        self.removed = {}

    async def start(self):
        try:
            for listener in self.server.servers:
                source = listener.sockets[0]
                sock = socket.socket(source.family, socket.SOCK_DGRAM)
                try:
                    if source.family == socket.AF_INET6:
                        sock.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
                    if hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
                        sock.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
                    sock.bind(source.getsockname())
                    sock.setblocking(False)
                    transport, _ = await asyncio.get_running_loop().create_datagram_endpoint(lambda: ServerDatagrams(self), sock=sock)
                    self.transports.append(transport)
                except BaseException:
                    sock.close()
                    raise
        except BaseException:
            self.close()
            raise

    def close(self):
        for transport in self.transports:
            transport.abort()
        self.transports.clear()
        self.peers.clear()

    def credential(self, player):
        return self.peers.setdefault(player, PeerStream()).key.hex()

    def publish(self, snapshot):
        if not isinstance(snapshot, dict) or set(snapshot) != {"tick", "elapsed_ms", "finished", "actors", "projectiles", "explosions", "acks"}:
            raise ProtocolError("Invalid host snapshot")
        validate_rows(snapshot["actors"], 32, 47)
        validate_rows(snapshot["projectiles"], 12, 256)
        validate_rows(snapshot["explosions"], 6, 48)
        tick = snapshot["tick"]
        if not integer(tick, 0, 2**32 - 1) or tick <= self.last_tick:
            return
        if not integer(snapshot["elapsed_ms"], 0, 2**32 - 1) or type(snapshot["finished"]) is not bool:
            raise ProtocolError("Invalid match state")
        acks = snapshot["acks"]
        if not isinstance(acks, list) or len(acks) > 32 or any(not isinstance(a, list) or len(a) != 2 or not integer(a[0], 1, 2**64 - 1) or not integer(a[1], 0, 2**32 - 1) for a in acks):
            raise ProtocolError("Invalid acknowledgements")
        self.last_tick = tick
        projectiles = {row[0]: row for row in snapshot["projectiles"]}
        for removed in self.active_projectiles - projectiles.keys():
            self.removed[removed] = tick
        self.active_projectiles = set(projectiles)
        horizon = self.setup["rules"]["physics_hz"] * 2
        self.removed = {key: value for key, value in self.removed.items() if tick - value <= horizon}
        self.removed = dict(list(self.removed.items())[-2048:])
        actors = {row[0]: row for row in snapshot["actors"]}
        explosions = {row[0]: row for row in snapshot["explosions"]}
        ack_map = dict(acks)
        active_slots = set(self.server.slots.values())
        budget = self.setup["sync"]["max_packet_bytes"]
        for player, peer in list(self.peers.items()):
            slot = self.server.slots.get(player)
            if peer.address is None or slot not in actors:
                continue
            peer.projectile_counts = {key: value for key, value in peer.projectile_counts.items() if key in projectiles}
            peer.explosion_counts = {key: value for key, value in peer.explosion_counts.items() if key in explosions}
            others = [row for index, row in actors.items() if index != slot and (index in active_slots or index >= self.setup["capacity"])]
            critical = [row for row in others if row[0] in peer.actor_status and peer.actor_status[row[0]] != (row[1], row[3])]
            rotated = [others[(peer.cursor + i) % len(others)] for i in range(len(others))] if others else []
            selected = []
            for row in critical + rotated:
                if row not in selected:
                    selected.append(row)
                if len(selected) >= self.setup["sync"]["actor_chunk"]:
                    break
            peer.cursor += len(selected)
            removed = list(self.removed)
            removed = [removed[(peer.removed_cursor + i) % len(removed)] for i in range(min(8, len(removed)))] if removed else []
            peer.removed_cursor += len(removed)
            shots = [p for key, p in projectiles.items() if p[2] or peer.projectile_counts.get(key, 0) < 2]
            shots.sort(key=lambda p: (0 if p[2] and p[10] == slot else 1 if peer.projectile_counts.get(p[0], 0) == 0 else 2 if p[2] else 3, peer.projectile_counts.get(p[0], 0), p[0]))
            effects = [e for key, e in explosions.items() if peer.explosion_counts.get(key, 0) < 2][:4]
            packet = {"t": tick, "e": snapshot["elapsed_ms"], "f": snapshot["finished"], "k": ack_map.get(player, 0),
                      "a": [actors[slot], *selected], "p": shots[:self.setup["sync"]["projectile_chunk"]], "x": effects, "r": removed}
            while True:
                body = compress_state(packet)
                if len(body) + HEADER.size + TAG <= budget:
                    break
                if packet["p"]:
                    packet["p"].pop()
                elif packet["x"]:
                    packet["x"].pop()
                elif len(packet["a"]) > 1:
                    packet["a"].pop()
                elif packet["r"]:
                    packet["r"].pop()
                else:
                    raise ProtocolError("Own aircraft exceeds state budget")
            for row in packet["p"]:
                peer.projectile_counts[row[0]] = peer.projectile_counts.get(row[0], 0) + 1
            for row in packet["a"]:
                peer.actor_status[row[0]] = (row[1], row[3])
            for row in packet["x"]:
                peer.explosion_counts[row[0]] = peer.explosion_counts.get(row[0], 0) + 1
            peer.transport.sendto(signed_packet(b"JG2S", self.setup["epoch"], player, body, peer.key), peer.address)
