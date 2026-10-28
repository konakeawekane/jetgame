"""Strict, versioned invitations. No public IP lookup service or router changes."""

import asyncio
import ipaddress
import re
import socket
from urllib.parse import parse_qs, urlencode, urlsplit
from .protocol import ProtocolError

MAX_LINK = 4096
MAX_ENDPOINTS = 8


def endpoint(host, port):
    return f"[{host}]:{port}" if ":" in host else f"{host}:{port}"


def split_endpoint(value):
    if not isinstance(value, str) or not 1 <= len(value) <= 255 or any(c.isspace() or ord(c) < 33 for c in value):
        raise ProtocolError("Enter HOST:PORT or a jetgame:// invitation")
    try:
        if value.startswith("["):
            closing = value.index("]:")
            host, port = value[1:closing], value[closing + 2:]
            ipaddress.IPv6Address(host)
        else:
            if value.count(":") != 1:
                raise ValueError()
            host, port = value.rsplit(":", 1)
            if not re.fullmatch(r"[A-Za-z0-9._-]+", host):
                raise ValueError()
        if not port.isascii() or not port.isdigit() or not 1 <= int(port) <= 65535:
            raise ValueError()
        address = ipaddress.ip_address(host.split("%", 1)[0]) if ":" in host or re.fullmatch(r"[0-9.]+", host) else None
        if address and (address.is_unspecified or address.is_multicast):
            raise ValueError()
        return host, int(port)
    except (ValueError, IndexError):
        raise ProtocolError("Invalid address; use HOST:PORT or [IPv6]:PORT") from None


def parse_link(value):
    if not isinstance(value, str) or len(value) > MAX_LINK:
        raise ProtocolError("Invitation exceeds size limit")
    value = value.strip()
    if not value.startswith("jetgame://"):
        split_endpoint(value)
        return [value], None
    try:
        parts = urlsplit(value)
        query = parse_qs(parts.query, strict_parsing=True, max_num_fields=12)
        addresses = query["endpoint"]
        match_id = query.get("id", [None])
        if parts.netloc != "join" or parts.path not in ("", "/") or parts.fragment or query.get("v") != ["1"] or set(query) - {"v", "id", "endpoint"}:
            raise ValueError()
        if not 1 <= len(addresses) <= MAX_ENDPOINTS or len(match_id) != 1 or (match_id[0] is not None and not re.fullmatch(r"[a-f0-9]{32}", match_id[0])):
            raise ValueError()
        for address in addresses:
            split_endpoint(address)
        return list(dict.fromkeys(addresses)), match_id[0]
    except (ValueError, KeyError):
        raise ProtocolError("Invalid or incompatible invitation") from None


def make_link(addresses, match_id):
    addresses = list(dict.fromkeys(addresses))[:MAX_ENDPOINTS]
    if not addresses:
        raise ProtocolError("No address is available to share")
    link = "jetgame://join?" + urlencode([("v", "1"), ("id", match_id)] + [("endpoint", a) for a in addresses])
    parse_link(link)
    return link


async def local_addresses():
    """OS interface/name resolution only. Never contact an Internet reflector."""
    loop = asyncio.get_running_loop()
    try:
        records = await asyncio.wait_for(loop.getaddrinfo(socket.gethostname(), None, type=socket.SOCK_STREAM), 2)
    except (OSError, TimeoutError):
        records = []
    addresses = []
    for _, _, _, _, sock in records:
        host = sock[0]
        address = ipaddress.ip_address(host.split("%", 1)[0])
        if address.is_loopback or address.is_link_local or address.is_multicast or address.is_unspecified:
            continue
        if host not in addresses:
            addresses.append(host)
    return sorted(addresses, key=lambda a: (":" not in a, a))


async def invitation(settings, port, match_id):
    local = list(await local_addresses())
    # A specific successful bind is useful even when hostname resolution omits it.
    for host in (settings.listen_ipv6, settings.listen_ipv4):
        if host and not ipaddress.ip_address(host).is_unspecified and host not in local:
            local.append(host)
    explicit = list(settings.public_addresses)
    for address in explicit:
        split_endpoint(address)
    bound = [a for a in local if (":" in a and settings.listen_ipv6 in ("::", a))
             or (":" not in a and settings.listen_ipv4 in ("0.0.0.0", a))]
    public = [endpoint(a, port) for a in bound if ipaddress.ip_address(a).is_global]
    public.sort(key=lambda a: not a.startswith("["))
    nearby = [endpoint(a, port) for a in bound if not ipaddress.ip_address(a).is_global
              and not ipaddress.ip_address(a).is_loopback]
    addresses = list(dict.fromkeys(public[:max(0, MAX_ENDPOINTS - len(explicit))] + explicit + nearby))[:MAX_ENDPOINTS]
    if not addresses:
        addresses = [endpoint(a, port) for a in bound if ipaddress.ip_address(a).is_loopback]
    if not addresses:
        addresses = [endpoint("::1" if settings.listen_ipv6 else "127.0.0.1", port)]
    scope = "local"
    for address in addresses:
        host, _ = split_endpoint(address)
        try:
            ip = ipaddress.ip_address(host)
        except ValueError:
            if host.lower() != "localhost":
                scope = "internet"  # Configured DNS candidate, not a reachability guarantee.
                break
            continue
        if ip.is_global:
            scope = "internet"
            break
        if not ip.is_loopback:
            scope = "lan"
    return make_link(addresses, match_id), scope
