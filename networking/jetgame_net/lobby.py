"""Transport-independent lobby state. Extend this for an authoritative game."""

import secrets
from .protocol import ProtocolError, clean_text


class Lobby:
    def __init__(self, name="DOGFIGHT", max_players=8, map_name="WORLD 01", mode="TEAM DOGFIGHT"):
        self.name = clean_text(name, "DOGFIGHT")
        self.max_players = max_players
        self.map_name = clean_text(map_name, "WORLD 01")
        self.mode = clean_text(mode, "TEAM DOGFIGHT")
        self.id = secrets.token_hex(16)
        self.admin_token = secrets.token_hex(24)
        self.players = {}
        self.next_id = 1

    def join(self, payload):
        if len(self.players) >= self.max_players:
            raise ProtocolError("Match is full")
        token = payload.get("admin_token", "")
        host = isinstance(token, str) and token.isascii() and secrets.compare_digest(token, self.admin_token)
        player_id = self.next_id
        self.next_id += 1
        self.players[player_id] = {"id": player_id, "name": clean_text(payload.get("name"), "PILOT", 24),
                                   "ready": host, "host": host}
        return player_id

    def welcome(self, player_id):
        return {"id": self.id, "name": self.name, "player_id": player_id,
                "host": self.players[player_id]["host"], "max_players": self.max_players,
                "map": self.map_name, "mode": self.mode, "capabilities": ["lobby"], "players": self.roster()}

    def roster(self):
        return list(self.players.values())

    def command(self, player_id, kind, payload):
        if kind == "ping":
            return "pong", {}
        if kind == "ready":
            if type(payload.get("ready")) is not bool:
                raise ProtocolError("ready must be boolean")
            self.players[player_id]["ready"] = payload["ready"]
            return None
        if kind == "start_match":
            host = self.players[player_id]["host"]
            return "error", {"code": "gameplay_not_implemented" if host else "host_required",
                             "message": "Online gameplay synchronization is not implemented" if host else "Only the host may start a match"}
        raise ProtocolError("Unsupported lobby request")
