# Hosting runtime contract

Python is server-only: the Rust client discovers/joins lobbies without invoking
Python. Every distributable should nevertheless include a private interpreter
because every copy may host a match.

On Windows the packaging script expects an extracted x64 CPython embeddable
distribution (3.11+), copied to `runtime/python/`. Keep the interpreter, standard
library ZIP, DLLs, restricted `._pth` file, and licenses together. The hosting
server uses only the standard library; pip and a global Python installation are
not required. No runtime download happens on game startup.

For development, debug builds may use `python` / `python3` from PATH. Override
with `--server-python ABSOLUTE_PATH` or `JETGAME_SERVER_PYTHON`. Release builds
require a bundled or explicitly configured hosting interpreter. Ordinary clients
still work when no Python interpreter exists.

The game starts its owned server without a console window, reads one bounded
readiness message, and retains a parent-lifetime pipe. Leaving or closing the app
closes that pipe; the server unregisters and exits. The worker also terminates
an unresponsive owned process. Other Python processes are never targeted.

This repository intentionally does not vendor a Python binary yet. Use
`scripts/package.ps1` with a supplied embeddable runtime and shared directory
endpoint; its bundle contract prevents accidentally shipping a client-only build.
