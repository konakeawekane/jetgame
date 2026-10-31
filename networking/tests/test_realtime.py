import asyncio
from pathlib import Path
import socket
import sys
from types import SimpleNamespace
import unittest
import zlib

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from jetgame_net.realtime import (HEADER, TAG, RUN, ClientDatagrams, GameRelay, ServerDatagrams, PeerStream,
                                 pack_inputs, unpack_inputs, signed_packet, open_packet, decompress_state)
from jetgame_net.protocol import ProtocolError


def actor(slot):
    row = [0] * 32
    row[0], row[1], row[2], row[3], row[4], row[5] = slot, 1, slot % 2, 1000, 800, 8
    row[10], row[19], row[23], row[27] = -1, 100000, -17000, 32767
    return row


def snapshot(tick):
    return {"tick": tick, "elapsed_ms": tick * 8, "finished": False,
            "actors": [actor(i) for i in range(47)],
            "projectiles": [[i, i % 47, i % 4 == 0, i * 5731, 12000 + i * 11, -i * 8231,
                             1000 + i, 2000 - i * 3, -85000, 2400, -1, 0] for i in range(1, 257)],
            "explosions": [[i, i * 533, 10000, -i * 31, 200, 2400] for i in range(1, 49)],
            "acks": [[1, tick], [2, tick]]}


class RealtimeTests(unittest.TestCase):
    def setUp(self):
        self.setup = {"epoch": 17, "capacity": 32, "rules": {"physics_hz": 120},
                      "sync": {"max_packet_bytes": 1200, "actor_chunk": 4, "projectile_chunk": 24}}
        self.server = SimpleNamespace(slots={1: 0, 2: 1})
        self.relay = GameRelay(self.server, self.setup, None)
        self.key = b"k" * 32

    def test_inputs_are_rle_batched_and_worst_case_stays_under_mtu(self):
        frames = [[0, 0, 0, 550, 0, 0]] * 18
        body = pack_inputs(21, frames)
        self.assertEqual(len(body), RUN.size)
        self.assertEqual(unpack_inputs(body), (21, frames))
        varied = [[i, -i, i, 550, i % 2, 0] for i in range(64)]
        body = pack_inputs(1, varied)
        self.assertLessEqual(HEADER.size + TAG + len(body), 1200)
        self.assertEqual(unpack_inputs(body), (1, varied))

    def test_corrupt_and_wrong_epoch_datagrams_are_rejected(self):
        packet = signed_packet(b"JG2I", 17, 2, pack_inputs(1, [[0, 0, 0, 550, 0, 0]]), self.key)
        for data, epoch in [(packet[:-1] + bytes([packet[-1] ^ 1]), 17), (packet, 18), (packet + b"x" * 1200, 17)]:
            with self.assertRaises(ProtocolError):
                open_packet(data, b"JG2I", epoch, 2, self.key)
        with self.assertRaises(ProtocolError):
            unpack_inputs(RUN.pack(1, 65, 0, 0, 0, 550, 0))
        with self.assertRaises(ProtocolError):
            unpack_inputs(RUN.pack(1, 1, 1001, 0, 0, 550, 0))

    def test_state_budget_prioritizes_own_aircraft_and_rotates_others(self):
        packets = []
        peer = PeerStream(key=self.key, address=("127.0.0.1", 12345),
                          transport=SimpleNamespace(sendto=lambda data, addr: packets.append(data)))
        self.relay.peers[1] = peer
        seen = set()
        for tick in range(6, 42, 6):
            state = snapshot(tick)
            # Snapshot rows contain integer flags, not JSON booleans.
            for row in state["projectiles"]:
                row[2] = int(row[2])
            self.relay.publish(state)
            data = packets[-1]
            self.assertLessEqual(len(data), 1200)
            packet = decompress_state(open_packet(data, b"JG2S", 17, 1, self.key))
            self.assertEqual(packet["a"][0][0], 0)
            self.assertEqual(packet["k"], tick)
            self.assertLessEqual(len(packet["a"]), 5)
            self.assertLess(len(packet["a"]), len(state["actors"]))
            seen.update(row[0] for row in packet["a"])
        self.assertTrue({0, 1, *range(32, 47)} <= seen)
        empty = snapshot(48)
        empty["projectiles"] = []
        empty["actors"][40][3] = 0
        self.relay.publish(empty)
        packet = decompress_state(open_packet(packets[-1], b"JG2S", 17, 1, self.key))
        self.assertTrue(packet["r"], "projectile removals must be repeated as tombstones")
        self.assertIn(40, [row[0] for row in packet["a"]], "death must take priority over rotation")

    def test_authenticated_inputs_are_rate_limited_and_spoofs_do_not_register(self):
        received = []
        self.relay.on_inputs = lambda **data: received.append(data)
        peer = PeerStream(key=self.key)
        self.relay.peers[2] = peer
        protocol = ServerDatagrams(self.relay)
        protocol.connection_made(SimpleNamespace())
        body = pack_inputs(1, [[0, 0, 0, 550, 1, 0]] * 6)
        packet = signed_packet(b"JG2I", 17, 2, body, self.key)
        protocol.datagram_received(packet[:-1] + bytes([packet[-1] ^ 1]), ("127.0.0.1", 9))
        self.assertIsNone(peer.address)
        for _ in range(80):
            protocol.datagram_received(packet, ("127.0.0.1", 9))
        self.assertEqual(len(received), 60)
        self.assertEqual(received[0]["player"], 2)
        self.assertEqual(len(received[0]["frames"]), 6)

    def test_state_decompression_cannot_expand_without_a_bound(self):
        with self.assertRaises(ProtocolError):
            decompress_state(zlib.compress(b"x" * 100000))


class EndpointTests(unittest.IsolatedAsyncioTestCase):
    async def test_gameplay_uses_external_tcp_port_without_contacting_any_service(self):
        receiver = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        receiver.bind(("127.0.0.1", 0))
        receiver.setblocking(False)
        values = {"peername": receiver.getsockname(), "socket": SimpleNamespace(family=socket.AF_INET)}
        writer = SimpleNamespace(get_extra_info=lambda key: values[key])
        client = None
        try:
            client = await ClientDatagrams.start(writer, {"epoch": 17}, 2, (b"k" * 32).hex(), lambda *a, **kw: None)
            data, _ = await asyncio.wait_for(asyncio.get_running_loop().sock_recvfrom(receiver, 1200), 2)
            self.assertEqual(open_packet(data, b"JG2I", 17, 2, b"k" * 32), b"")
        finally:
            if client:
                client.close()
                await client.wait_closed()
            receiver.close()
