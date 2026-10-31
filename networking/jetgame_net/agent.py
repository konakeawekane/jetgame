"""Local application service. Rust sees commands/events, never network packets."""

import asyncio
import contextlib
from .client import Connection
from .discovery import discover
from .protocol import ProtocolError
from .server import MatchServer


class NetworkService:
    def __init__(self, settings, emit):
        self.settings, self.output = settings, emit
        self.op = 0
        self.pending = None
        self.watcher = None
        self.connection = None
        self.hosted = None

    def emit(self, event, **payload):
        self.output({"event": event, "op": self.op, **payload})

    async def leave(self):
        # Close the stream before cancelling its reader: pending read/drain operations
        # must wake even when cancellation races with an already-buffered message.
        if self.connection:
            self.connection.writer.close()
        for task in (self.pending, self.watcher):
            if task:
                task.cancel()
                with contextlib.suppress(asyncio.CancelledError):
                    await task
        self.pending = self.watcher = None
        if self.connection:
            await self.connection.close()
            self.connection = None
        if self.hosted:
            await self.hosted.close()
            self.hosted = None

    async def command(self, command):
        kind, op = command.get("cmd"), command.get("op", 0)
        allowed = {"discover": {"cmd", "op"}, "host": {"cmd", "op", "options"},
                   "join": {"cmd", "op", "endpoint"}, "ready": {"cmd", "op", "ready"},
                   "start": {"cmd", "op"}, "leave": {"cmd"}, "shutdown": {"cmd"}}
        allowed.update({"begin": {"cmd", "op", "setup"}, "inputs": {"cmd", "op", "epoch", "first", "frames"},
                        "state": {"cmd", "op", "epoch", "snapshot"}, "action": {"cmd", "op", "epoch", "action_id", "action", "team"},
                        "action_result": {"cmd", "op", "epoch", "player", "action_id", "accepted", "reason"}, "return_lobby": {"cmd", "op", "epoch"}})
        if kind not in allowed or set(command) != allowed[kind] or type(op) is not int or not 0 <= op < 2**64:
            raise ProtocolError("Invalid application command")
        if kind in ("leave", "shutdown"):
            await self.leave()
            return
        if kind in ("host", "join", "discover"):
            await self.leave()
            self.op = op
            self.pending = asyncio.create_task(self.begin(command))
        elif self.connection and op == self.op:
            if kind in ("begin", "state", "action_result", "return_lobby"):
                if not self.hosted or not self.connection.info["host"]:
                    raise ProtocolError("Host authority is required")
                if kind == "begin":
                    await self.hosted.begin(command["setup"])
                elif kind == "state":
                    if self.hosted.game and self.hosted.game.setup["epoch"] == command["epoch"]:
                        self.hosted.game.publish(command["snapshot"])
                elif kind == "action_result":
                    await self.hosted.action_result(command["epoch"], command["player"], command["action_id"], command["accepted"], command["reason"])
                else:
                    if self.hosted.game and self.hosted.game.setup["epoch"] == command["epoch"]:
                        await self.hosted.return_lobby()
                return
            if kind == "inputs":
                if self.connection.game and self.connection.game.epoch == command["epoch"]:
                    self.connection.game.send_inputs(command["first"], command["frames"])
                return
            if kind == "action":
                await self.connection.send("game_action", {key: command[key] for key in ("epoch", "action", "action_id", "team")})
                return
            if kind == "ready" and type(command["ready"]) is not bool:
                raise ProtocolError("Invalid Ready state")
            await self.connection.send("ready" if kind == "ready" else "start_match",
                                       {"ready": command["ready"]} if kind == "ready" else {})

    async def begin(self, command):
        try:
            kind = command["cmd"]
            if kind == "discover":
                self.emit("servers", servers=await discover(self.settings))
                return
            link = None
            scope = "local"
            if kind == "host":
                options = command["options"]
                if not isinstance(options, dict) or set(options) - {"name", "max_players", "map", "mode"}:
                    raise ProtocolError("Invalid host options")
                self.hosted = MatchServer(options.get("name", "DOGFIGHT"), options.get("max_players", 8),
                                          options.get("map", "WORLD 01"), options.get("mode", "TEAM DOGFIGHT"), self.settings)
                self.hosted.on_inputs = lambda **payload: self.emit("inputs", **payload)
                await self.hosted.start()
                link, scope = await self.hosted.invitation()
                address = self.hosted.local_endpoint
                token = self.hosted.admin_token
            else:
                address, token = command["endpoint"], ""
            self.connection = await Connection.join(address, self.settings, token)
            self.emit("connected", info=self.connection.info, endpoint=self.connection.endpoint)
            if link:
                self.emit("share_link", link=link, scope=scope,
                          warning=self.hosted.discovery_warning)
            self.watcher = asyncio.create_task(self.watch())
        except (OSError, ValueError, TimeoutError) as error:
            if self.connection:
                await self.connection.close()
                self.connection = None
            if self.hosted:
                await self.hosted.close()
                self.hosted = None
            self.emit("error", message=str(error)[:240])

    async def watch(self):
        try:
            await self.connection.listen(self.emit)
        except (OSError, ValueError, TimeoutError, asyncio.IncompleteReadError) as error:
            reason = "Host disconnected" if isinstance(error, asyncio.IncompleteReadError) else "Connection timed out" if isinstance(error, TimeoutError) else str(error)[:160]
            self.emit("disconnected", reason=reason or "Connection closed")
            await self.connection.close()
            self.connection = None
            if self.hosted:
                await self.hosted.close()
                self.hosted = None
