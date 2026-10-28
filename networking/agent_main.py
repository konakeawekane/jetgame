"""Bounded JSON-line IPC over owned pipes. No local TCP listener or Python embedding."""

import asyncio
import concurrent.futures
import contextlib
import json
from pathlib import Path
import queue
import sys
import threading

sys.path.insert(0, str(Path(__file__).resolve().parent))
from jetgame_net.agent import NetworkService
from jetgame_net.config import NetworkSettings

MAX_IPC = 65536


async def run():
    loop = asyncio.get_running_loop()
    inbox = asyncio.Queue(16)
    output = queue.Queue(64)
    stopped = asyncio.Event()

    def emit(event):
        try:
            line = json.dumps(event, separators=(",", ":"), allow_nan=False).encode("utf-8") + b"\n"
            if len(line) > MAX_IPC:
                raise ValueError("IPC event too large")
            output.put_nowait(line)
        except (ValueError, queue.Full):
            stopped.set()

    def read_commands():
        try:
            while True:
                line = sys.stdin.buffer.readline(MAX_IPC + 1)
                if not line or len(line) > MAX_IPC:
                    break
                future = asyncio.run_coroutine_threadsafe(inbox.put(line), loop)
                future.result(timeout=5)
        except (OSError, RuntimeError, concurrent.futures.TimeoutError):
            pass
        finally:
            with contextlib.suppress(RuntimeError):
                loop.call_soon_threadsafe(stopped.set)

    def write_events():
        try:
            while True:
                line = output.get()
                if line is None:
                    return
                sys.stdout.buffer.write(line)
                sys.stdout.buffer.flush()
        except OSError:
            with contextlib.suppress(RuntimeError):
                loop.call_soon_threadsafe(stopped.set)

    threading.Thread(target=read_commands, daemon=True).start()
    threading.Thread(target=write_events, daemon=True).start()
    service = None
    try:
        while not stopped.is_set():
            get = asyncio.create_task(inbox.get())
            stop = asyncio.create_task(stopped.wait())
            done, _ = await asyncio.wait((get, stop), return_when=asyncio.FIRST_COMPLETED)
            if stop in done:
                get.cancel()
                with contextlib.suppress(asyncio.CancelledError):
                    await get
                break
            stop.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await stop
            command = None
            try:
                command = json.loads(get.result())
                if not isinstance(command, dict):
                    raise ValueError("Invalid IPC command")
                if service is None:
                    if set(command) != {"cmd", "settings"} or command["cmd"] != "configure":
                        raise ValueError("Configuration handshake required")
                    service = NetworkService(NetworkSettings.parse(command["settings"]), emit)
                else:
                    await service.command(command)
                    if command["cmd"] == "shutdown":
                        break
            except (ValueError, OSError, TypeError, TimeoutError) as error:
                op = command.get("op", 0) if isinstance(command, dict) else 0
                emit({"event": "error", "op": op if type(op) is int and 0 <= op < 2**64 else 0,
                      "message": str(error)[:240]})
                if service is None:
                    break
    finally:
        if service:
            await service.leave()
        with contextlib.suppress(queue.Full):
            output.put_nowait(None)
        # Let the bounded writer flush final events before the process exits.
        await asyncio.sleep(0.02)


if __name__ == "__main__":
    try:
        asyncio.run(run())
    except KeyboardInterrupt:
        pass
