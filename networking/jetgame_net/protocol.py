"""Bounded TCP control frames for discovery-independent lobby and match lifecycle."""

import asyncio
import json
import struct

VERSION = 1
MAX_FRAME = 65536


class ProtocolError(ValueError):
    pass


def encode(kind, payload=None, request_id=0):
    body = json.dumps({"version": VERSION, "kind": kind, "request_id": request_id,
                       "payload": payload or {}}, separators=(",", ":"), allow_nan=False).encode("utf-8")
    if not 0 < len(body) <= MAX_FRAME:
        raise ProtocolError("frame size outside limit")
    return struct.pack("!I", len(body)) + body


async def receive(reader, timeout=10):
    async def read():
        length = struct.unpack("!I", await reader.readexactly(4))[0]
        if not 0 < length <= MAX_FRAME:
            raise ProtocolError("frame size outside limit")
        try:
            message = json.loads((await reader.readexactly(length)).decode("utf-8"),
                                 parse_constant=lambda _: (_ for _ in ()).throw(ProtocolError("nonfinite JSON")))
        except (UnicodeError, json.JSONDecodeError, RecursionError) as error:
            raise ProtocolError("invalid JSON") from error
        if not isinstance(message, dict) or type(message.get("version")) is not int or message.get("version") != VERSION:
            raise ProtocolError("incompatible protocol version")
        if set(message) != {"version", "kind", "request_id", "payload"}:
            raise ProtocolError("invalid message fields")
        if not isinstance(message.get("kind"), str) or not 1 <= len(message["kind"]) <= 40 or not isinstance(message.get("payload"), dict):
            raise ProtocolError("invalid message envelope")
        if type(message.get("request_id")) is not int or not 0 <= message["request_id"] < 2**64:
            raise ProtocolError("invalid request ID")
        return message
    return await asyncio.wait_for(read(), timeout)


async def send(writer, kind, payload=None, request_id=0):
    writer.write(encode(kind, payload, request_id))
    await asyncio.wait_for(writer.drain(), 2)


def clean_text(value, default, limit=32):
    if not isinstance(value, str):
        return default
    return "".join(c for c in value if c.isascii() and c.isprintable()).strip()[:limit] or default
