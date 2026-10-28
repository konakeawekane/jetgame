import asyncio
import json
from pathlib import Path
import struct
import sys
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from jetgame_net.directory import Directory
from jetgame_net.protocol import MAX_FRAME, ProtocolError, encode, receive, request, send
from jetgame_net.server import MatchServer


class NetworkTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.directory = Directory()
        port = await self.directory.start(port=0)
        self.endpoint = f"127.0.0.1:{port}"
        self.server = MatchServer("TEST MATCH", 2, directory=self.endpoint)
        self.port = await self.server.start(port=0)
        self.clients = []

    async def asyncTearDown(self):
        for _, writer in self.clients:
            writer.close()
            await writer.wait_closed()
        await self.server.close()
        await self.directory.close()

    async def join(self, token=""):
        reader, writer = await asyncio.open_connection("127.0.0.1", self.port)
        self.clients.append((reader, writer))
        await send(writer, "join", {"name": "PILOT", "admin_token": token}, 1)
        return reader, writer, await receive(reader)

    async def until_kind(self, reader, kind):
        for _ in range(10):
            message = await receive(reader, 2)
            if message["kind"] == kind:
                return message
        self.fail(f"missing {kind}")

    async def test_discovery_registration_and_shutdown_removal(self):
        listings = []
        for _ in range(100):
            listings = (await request(self.endpoint, "discover"))["servers"]
            if listings:
                break
            await asyncio.sleep(0.02)
        self.assertEqual(listings[0]["endpoint"], f"127.0.0.1:{self.port}")
        self.assertEqual(listings[0]["reachability"], "unverified")
        await self.server.close()
        self.assertEqual((await request(self.endpoint, "discover"))["servers"], [])

    async def test_two_clients_roster_ready_ping_and_host_permissions(self):
        host_reader, host_writer, welcome = await self.join(self.server.admin_token)
        self.assertTrue(welcome["payload"]["host"])
        guest_reader, guest_writer, guest_welcome = await self.join()
        self.assertFalse(guest_welcome["payload"]["host"])
        await self.until_kind(guest_reader, "lobby")
        await send(guest_writer, "ready", {"ready": True})
        roster = await self.until_kind(guest_reader, "lobby")
        self.assertEqual(len(roster["payload"]["players"]), 2)
        self.assertTrue(all(player["ready"] for player in roster["payload"]["players"]))
        await send(guest_writer, "ping", {}, 42)
        pong = await self.until_kind(guest_reader, "pong")
        self.assertEqual(pong["request_id"], 42)
        await send(guest_writer, "start_match")
        self.assertEqual((await self.until_kind(guest_reader, "error"))["payload"]["code"], "host_required")
        await send(host_writer, "start_match")
        self.assertEqual((await self.until_kind(host_reader, "error"))["payload"]["code"], "gameplay_not_implemented")

    async def test_full_server_rejects_extra_player(self):
        await self.join()
        await self.join()
        _, _, reply = await self.join()
        self.assertEqual(reply["kind"], "error")
        self.assertIn("full", reply["payload"]["message"])

    async def test_leases_expire_and_wrong_tokens_cannot_modify_listing(self):
        registration = await request(self.endpoint, "register", {"port": 12345, "name": "MANUAL", "max_players": 8})
        with self.assertRaises(ProtocolError):
            await request(self.endpoint, "unregister", {"id": registration["id"], "lease_token": "wrong"})
        self.directory.matches[registration["id"]]["expires"] = time.monotonic() - 1
        listings = (await request(self.endpoint, "discover"))["servers"]
        self.assertFalse(any(match["id"] == registration["id"] for match in listings))

    async def test_framing_rejects_bad_versions_and_oversized_packets(self):
        for packet in [struct.pack("!I", MAX_FRAME + 1),
                       self.raw({"version": 99, "kind": "ping", "request_id": 0, "payload": {}}),
                       self.raw({"version": True, "kind": "ping", "request_id": 0, "payload": {}})]:
            reader = asyncio.StreamReader()
            reader.feed_data(packet)
            reader.feed_eof()
            with self.assertRaises(ProtocolError):
                await receive(reader)

    async def test_fragmented_frame_is_reassembled(self):
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

    async def test_host_process_exits_after_parent_pipe_closes(self):
        entry = Path(__file__).resolve().parents[1] / "server_main.py"
        process = await asyncio.create_subprocess_exec(sys.executable, "-I", "-u", str(entry),
            "--parent-stdin", "--port", "0", stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        try:
            ready = json.loads(await asyncio.wait_for(process.stdout.readline(), 4))
            self.assertEqual(ready["kind"], "ready")
            process.stdin.close()
            await asyncio.wait_for(process.wait(), 4)
            self.assertEqual(process.returncode, 0)
        finally:
            if process.returncode is None:
                process.kill()
                await process.wait()


if __name__ == "__main__":
    unittest.main()
