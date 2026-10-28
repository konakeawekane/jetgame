import asyncio
from dataclasses import replace
import json
from pathlib import Path
import socket
import struct
import sys
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "networking"))
from jetgame_net.client import Connection, resolve_candidates
from jetgame_net.config import NetworkSettings
from jetgame_net.discovery import discover
from jetgame_net.links import invitation, make_link, parse_link, split_endpoint
from jetgame_net.protocol import MAX_FRAME, ProtocolError, encode, receive
from jetgame_net.server import MatchServer


def unused_udp_port():
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class NetworkTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.settings = NetworkSettings.parse({"host_port": 0, "listen_ipv6": "", "listen_ipv4": "127.0.0.1", "lan_enabled": False})
        self.server = MatchServer("TEST MATCH", 2, settings=self.settings)
        await self.server.start()
        self.connections = []

    async def asyncTearDown(self):
        for connection in self.connections:
            await connection.close()
        await self.server.close()

    async def join(self, owner=False, address=None):
        client = await Connection.join(address or self.server.local_endpoint, self.settings,
                                       self.server.admin_token if owner else "")
        self.connections.append(client)
        return client

    async def until_kind(self, client, kind):
        for _ in range(10):
            message = await receive(client.reader, 2)
            if message["kind"] == kind:
                return message
        self.fail(f"Missing {kind}")

    async def test_roster_ready_and_host_permissions(self):
        host, guest = await self.join(True), await self.join()
        self.assertTrue(host.info["host"])
        self.assertFalse(guest.info["host"])
        await self.until_kind(guest, "lobby")
        await guest.send("ready", {"ready": True})
        roster = (await self.until_kind(guest, "lobby"))["payload"]["players"]
        self.assertEqual(len(roster), 2)
        self.assertTrue(all(p["ready"] for p in roster))
        await guest.send("start_match")
        self.assertEqual((await self.until_kind(guest, "error"))["payload"]["code"], "host_required")
        await host.send("start_match")
        self.assertEqual((await self.until_kind(host, "error"))["payload"]["code"], "gameplay_not_implemented")

    async def test_capacity_and_malformed_commands_do_not_kill_server(self):
        first = await self.join()
        await self.join()
        with self.assertRaisesRegex(ProtocolError, "full"):
            await self.join()
        await first.send("ready", {"ready": "yes"})
        self.assertIn("boolean", (await self.until_kind(first, "error"))["payload"]["message"])

    async def test_ipv6_is_tried_before_ipv4_and_ipv4_fallback_connects(self):
        link = make_link([f"127.0.0.1:{self.server.port}", f"[::1]:{self.server.port}"], self.server.lobby.id)
        candidates = await resolve_candidates(parse_link(link)[0], self.settings)
        self.assertEqual(candidates[0][0], socket.AF_INET6)
        attempted = []
        original = asyncio.open_connection
        async def tracked(host, port, **kwargs):
            attempted.append(kwargs["family"])
            return await original(host, port, **kwargs)
        with patch("jetgame_net.client.asyncio.open_connection", tracked):
            client = await self.join(address=link)
        self.assertEqual(attempted, [socket.AF_INET6, socket.AF_INET])
        self.assertEqual(client.info["name"], "TEST MATCH")

    async def test_native_ipv6_listener_accepts_connections(self):
        settings = replace(self.settings, listen_ipv4="", listen_ipv6="::1")
        server = MatchServer(settings=settings)
        try:
            await server.start()
        except OSError:
            self.skipTest("IPv6 is not enabled on this machine")
        try:
            connection = await Connection.join(server.local_endpoint, settings)
            await connection.close()
        finally:
            await server.close()

    async def test_wrong_match_invitation_is_rejected(self):
        link = make_link([self.server.local_endpoint], "0" * 32)
        with self.assertRaisesRegex(ProtocolError, "different match"):
            await self.join(address=link)

    async def test_incomplete_handshake_falls_back_without_killing_client(self):
        async def incomplete(reader, writer):
            writer.write(b"\x00\x00")
            await writer.drain()
            writer.close()
            await writer.wait_closed()
        broken = await asyncio.start_server(incomplete, "127.0.0.1", 0)
        try:
            port = broken.sockets[0].getsockname()[1]
            link = make_link([f"127.0.0.1:{port}", self.server.local_endpoint], self.server.lobby.id)
            client = await self.join(address=link)
            self.assertEqual(client.endpoint, self.server.local_endpoint)
        finally:
            broken.close()
            await broken.wait_closed()

    async def test_lan_multicast_discovers_multiple_hosts_and_removes_closed_host(self):
        settings = replace(self.settings, lan_enabled=True, listen_ipv4="0.0.0.0", discovery_port=unused_udp_port(),
                           discovery_interface="0.0.0.0", discovery_seconds=0.35)
        first, second = MatchServer("FIRST", settings=settings), MatchServer("SECOND", settings=settings)
        try:
            await first.start()
            await second.start()
            self.assertFalse(first.discovery_warning or second.discovery_warning)
            listings = await discover(settings)
            self.assertEqual({s["name"] for s in listings}, {"FIRST", "SECOND"})
            self.assertTrue(all(s["reachability"] == "LAN" for s in listings))
            client = await Connection.join(listings[0]["endpoint"], settings)
            self.assertEqual(client.info["name"], listings[0]["name"])
            await client.close()
            await first.close()
            self.assertEqual([s["name"] for s in await discover(settings)], ["SECOND"])
        finally:
            await first.close()
            await second.close()

    async def test_generated_invite_prefers_global_ipv6_and_keeps_explicit_ipv4(self):
        # TEST-NET addresses are parsed only; no connection is made to them.
        settings = replace(self.settings, listen_ipv6="::", public_addresses=["198.51.100.7:40000"])
        with patch("jetgame_net.links.local_addresses", return_value=["2001:4860::1234", "192.168.1.5"]):
            link, scope = await invitation(settings, 29010, self.server.lobby.id)
        addresses, _ = parse_link(link)
        self.assertEqual(addresses, ["[2001:4860::1234]:29010", "198.51.100.7:40000"])
        self.assertEqual(scope, "internet")

    async def test_links_are_generated_automatically_and_distinguish_network_scope(self):
        for addresses, listen_ipv4, scope in [(["1.1.1.1"], "0.0.0.0", "internet"),
                                             (["192.168.1.5"], "0.0.0.0", "lan"),
                                             ([], "127.0.0.1", "local")]:
            settings = replace(self.settings, listen_ipv4=listen_ipv4)
            with patch("jetgame_net.links.local_addresses", return_value=addresses):
                link, actual_scope = await invitation(settings, self.server.port, self.server.lobby.id)
            candidates, match_id = parse_link(link)
            self.assertEqual(actual_scope, scope)
            self.assertEqual(match_id, self.server.lobby.id)
            self.assertEqual(candidates, [f"{addresses[0] if addresses else '127.0.0.1'}:{self.server.port}"])

    async def test_unavailable_ipv6_listener_is_not_in_generated_link(self):
        blocker = socket.socket(socket.AF_INET6, socket.SOCK_STREAM)
        server = None
        try:
            blocker.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
            blocker.bind(("::1", 0))
            blocker.listen(1)
            port = blocker.getsockname()[1]
            server = MatchServer(settings=replace(self.settings, host_port=port, listen_ipv6="::1"))
            await server.start()
            link, scope = await server.invitation()
            addresses, _ = parse_link(link)
            self.assertEqual(addresses, [f"127.0.0.1:{port}"])
            self.assertEqual(scope, "local")
            connection = await Connection.join(link, self.settings)
            await connection.close()
        finally:
            if server:
                await server.close()
            blocker.close()

    async def test_protocol_rejects_bad_frames_and_handles_fragmentation(self):
        for frame in [struct.pack("!I", MAX_FRAME + 1), self.raw({"version": True, "kind": "ping", "request_id": 0, "payload": {}})]:
            reader = asyncio.StreamReader()
            reader.feed_data(frame)
            reader.feed_eof()
            with self.assertRaises(ProtocolError):
                await receive(reader)
        reader = asyncio.StreamReader()
        frame = encode("ping", {}, 8)
        task = asyncio.create_task(receive(reader))
        reader.feed_data(frame[:3])
        await asyncio.sleep(0)
        self.assertFalse(task.done())
        reader.feed_data(frame[3:])
        self.assertEqual((await task)["request_id"], 8)

    @staticmethod
    def raw(value):
        body = json.dumps(value).encode()
        return struct.pack("!I", len(body)) + body

    async def test_agent_pipe_eof_closes_owned_host(self):
        process = await asyncio.create_subprocess_exec(sys.executable, "-I", "-u", str(ROOT / "networking/agent_main.py"),
                                                       stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        try:
            settings = {"host_port": 0, "listen_ipv6": "", "listen_ipv4": "127.0.0.1", "lan_enabled": False}
            commands = [{"cmd": "configure", "settings": settings}, {"cmd": "host", "op": 1, "options": {"name": "PIPE HOST", "max_players": 2}}]
            for command in commands:
                process.stdin.write(json.dumps(command).encode() + b"\n")
            await process.stdin.drain()
            ready = json.loads(await asyncio.wait_for(process.stdout.readline(), 5))
            self.assertEqual(ready["event"], "connected", ready)
            process.stdin.close()
            await asyncio.wait_for(process.wait(), 5)
            self.assertEqual(process.returncode, 0, (await process.stderr.read()).decode())
            host, port = split_endpoint(ready["endpoint"])
            with self.assertRaises(OSError):
                await asyncio.open_connection(host, port)
        finally:
            if process.returncode is None:
                process.kill()
                await process.wait()

    async def test_standalone_host_shares_server_implementation_and_closes_on_eof(self):
        process = await asyncio.create_subprocess_exec(sys.executable, "-I", "-u", str(ROOT / "networking/server_main.py"),
                                                       "--port", "0", "--no-lan", "--parent-stdin", "--name", "HEADLESS",
                                                       stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        client = None
        try:
            ready = json.loads(await asyncio.wait_for(process.stdout.readline(), 5))
            self.assertEqual(ready["kind"], "ready")
            parse_link(ready["link"])
            client = await Connection.join(ready["endpoint"], self.settings)
            self.assertEqual(client.info["name"], "HEADLESS")
            process.stdin.close()
            await asyncio.wait_for(process.wait(), 5)
            self.assertEqual(process.returncode, 0, (await process.stderr.read()).decode())
            await self.until_kind(client, "lobby")
            with self.assertRaises(asyncio.IncompleteReadError):
                await receive(client.reader, 2)
        finally:
            if client:
                await client.close()
            if process.returncode is None:
                process.kill()
                await process.wait()


class PolicyTests(unittest.TestCase):
    def test_strict_invitation_parser(self):
        for link in ["jetgame://evil?v=1&endpoint=localhost:80", "jetgame://join?v=2&endpoint=localhost:80",
                     "jetgame://join?v=1&endpoint=localhost:80&execute=file", "0.0.0.0:29010", "example.com:0",
                     "[::]:29010", "example.com:29010/path", "x" * 4097]:
            with self.assertRaises(ProtocolError, msg=link):
                parse_link(link)

    def test_configuration_rejects_invalid_limits_and_obsolete_directory(self):
        for values in [{"host_port": -1}, {"idle_timeout": 2}, {"max_players": 500}, {"discovery_group": "1.2.3.4"},
                       {"directory": "example.com:29000"}, {"lan_enabled": "yes"}, {"connect_timeout": float("nan")}]:
            with self.assertRaises((ValueError, TypeError)):
                NetworkSettings.parse(values)


if __name__ == "__main__":
    unittest.main()
