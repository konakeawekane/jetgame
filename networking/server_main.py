"""Isolated entry point for standalone and game-launched headless servers."""

import argparse
import asyncio
import json
import logging
from pathlib import Path
import sys
import threading

# Also works with Python -I and a minimal embeddable runtime's restricted search path.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from jetgame_net.directory import Directory
from jetgame_net.server import MatchServer


async def run(args):
    stopped = asyncio.Event()
    loop = asyncio.get_running_loop()
    if args.parent_stdin:
        def parent_watch():
            while sys.stdin.buffer.read(1):
                pass
            if not loop.is_closed():
                loop.call_soon_threadsafe(stopped.set)
        threading.Thread(target=parent_watch, daemon=True).start()
    service = Directory() if args.service == "directory" else MatchServer(
        args.name, args.max_players, args.map, args.mode, args.directory, args.public_port)
    try:
        port = await service.start(args.bind, args.port)
        ready = {"kind": "ready", "port": port}
        if isinstance(service, MatchServer):
            ready["admin_token"] = service.admin_token
        # stdout is only the parent bootstrap channel; never write the token to logs.
        print(json.dumps(ready), flush=True)
        await stopped.wait()
    finally:
        await service.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--service", choices=["match", "directory"], default="match")
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--name", default="DOGFIGHT")
    parser.add_argument("--max-players", type=int, choices=range(2, 33), default=8)
    parser.add_argument("--map", default="WORLD 01")
    parser.add_argument("--mode", default="TEAM DOGFIGHT")
    parser.add_argument("--directory")
    parser.add_argument("--public-port", type=int)
    parser.add_argument("--parent-stdin", action="store_true")
    args = parser.parse_args()
    if not 0 <= args.port <= 65535 or (args.public_port is not None and not 1 <= args.public_port <= 65535):
        parser.error("invalid port")
    logging.basicConfig(level=logging.INFO, stream=sys.stderr)
    try:
        asyncio.run(run(args))
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
