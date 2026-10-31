"""Standalone headless host using the same server/config as the game-launched agent."""

import argparse
import asyncio
import json
import os
from pathlib import Path
import sys
import threading

sys.path.insert(0, str(Path(__file__).resolve().parent))
from jetgame_net.config import load
from jetgame_net.server import MatchServer


async def run(args):
    settings = load(args.config)
    updates = {}
    if args.port is not None:
        updates["host_port"] = args.port
    if args.no_lan:
        updates["lan_enabled"] = False
    if args.public_address:
        updates["public_addresses"] = args.public_address
    from dataclasses import asdict
    settings = type(settings).parse({**asdict(settings), **updates})
    server = MatchServer(args.name, args.max_players, args.map, args.mode, settings)
    stopped = asyncio.Event()
    loop = asyncio.get_running_loop()
    if args.parent_stdin:
        def watch():
            while os.read(sys.stdin.fileno(), 1):
                pass
            if not loop.is_closed():
                loop.call_soon_threadsafe(stopped.set)
        threading.Thread(target=watch, daemon=True).start()
    try:
        await server.start()
        link, scope = await server.invitation()
        print(json.dumps({"kind": "ready", "port": server.port, "endpoint": server.local_endpoint,
                          "link": link, "scope": scope, "warning": server.discovery_warning}), flush=True)
        await stopped.wait()
    finally:
        await server.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config")
    parser.add_argument("--port", type=int)
    parser.add_argument("--name", default="DOGFIGHT")
    parser.add_argument("--max-players", type=int, default=8)
    parser.add_argument("--map", default="WORLD 01")
    parser.add_argument("--mode", default="TEAM DOGFIGHT")
    parser.add_argument("--public-address", action="append")
    parser.add_argument("--no-lan", action="store_true")
    parser.add_argument("--parent-stdin", action="store_true")
    try:
        asyncio.run(run(parser.parse_args()))
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
