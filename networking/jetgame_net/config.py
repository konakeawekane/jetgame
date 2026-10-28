"""Shared transport policy for embedded hosts, standalone servers, and clients."""

from dataclasses import dataclass, field, fields
import ipaddress
import json
from pathlib import Path


@dataclass(frozen=True)
class NetworkSettings:
    host_port: int = 29010
    listen_ipv6: str = "::"
    listen_ipv4: str = "0.0.0.0"
    lan_enabled: bool = True
    discovery_port: int = 29009
    discovery_group: str = "239.255.42.99"
    discovery_interface: str = "0.0.0.0"
    discovery_seconds: float = 1.2
    connect_timeout: float = 2.0
    handshake_timeout: float = 3.0
    idle_timeout: float = 10.0
    ping_seconds: float = 2.0
    max_connections: int = 64
    max_players: int = 32
    max_listings: int = 64
    public_addresses: list = field(default_factory=list)
    player_name: str = "PILOT"

    @classmethod
    def parse(cls, values=None):
        values = dict(values or {})
        # Launcher settings belong to Rust; network policy belongs here.
        for key in ("python", "agent_script"):
            values.pop(key, None)
        names = {f.name for f in fields(cls)}
        unknown = set(values) - names
        if unknown:
            raise ValueError(f"Unknown networking settings: {', '.join(sorted(unknown))}")
        result = cls(**values)
        for key, low, high in (("host_port", 0, 65535), ("discovery_port", 1, 65535),
                               ("max_connections", 2, 256), ("max_players", 2, 32),
                               ("max_listings", 1, 128)):
            value = getattr(result, key)
            if type(value) is not int or not low <= value <= high:
                raise ValueError(f"Invalid {key}")
        for key in ("discovery_seconds", "connect_timeout", "handshake_timeout", "idle_timeout", "ping_seconds"):
            value = getattr(result, key)
            if type(value) not in (int, float) or not 0.05 <= value <= 60:
                raise ValueError(f"Invalid {key}")
        if result.ping_seconds * 2 >= result.idle_timeout:
            raise ValueError("idle_timeout must exceed two ping intervals")
        if type(result.lan_enabled) is not bool:
            raise ValueError("Invalid lan_enabled")
        if not ipaddress.IPv4Address(result.discovery_group).is_multicast:
            raise ValueError("discovery_group must be IPv4 multicast")
        ipaddress.IPv4Address(result.discovery_interface)
        if not isinstance(result.public_addresses, list) or len(result.public_addresses) > 8 or any(not isinstance(a, str) or len(a) > 255 for a in result.public_addresses):
            raise ValueError("Invalid public_addresses")
        if not isinstance(result.player_name, str) or not 1 <= len(result.player_name) <= 24:
            raise ValueError("Invalid player_name")
        for key, family in (("listen_ipv4", 4), ("listen_ipv6", 6)):
            value = getattr(result, key)
            if not isinstance(value, str) or (value and ipaddress.ip_address(value).version != family):
                raise ValueError(f"Invalid {key}")
        if not result.listen_ipv4 and not result.listen_ipv6:
            raise ValueError("At least one listen address is required")
        return result


def load(path=None):
    return NetworkSettings.parse(json.loads(Path(path).read_text(encoding="utf-8-sig")) if path else {})
