# Python runtime

Each network-enabled game copy needs a private Python 3.11+ interpreter. Python
owns discovery, joining, and hosting; Rust owns simulation, rendering, and the UI.
Offline bot preview does not start Python.

On Windows `scripts/package.ps1` accepts an extracted x64 CPython embeddable
distribution and copies it to `runtime/python/`. Keep its standard-library ZIP,
DLLs, restricted `._pth`, and licenses together. No pip packages or startup runtime
downloads are required. Both Python entry points add their own source directory
to the restricted import path explicitly.

Debug builds may use Python on PATH. Override with `--python ABSOLUTE_PATH`,
`JETGAME_PYTHON`, or `python` in `networking.local.json`. Release networking
requires the bundled or explicitly configured interpreter. The runtime process
is hidden on Windows and communicates with Rust over owned pipes. Closing the
app closes its stdin and triggers cleanup; an unresponsive owned process is
terminated. Other Python processes are never targeted.

This repository does not contain the interpreter binary. Packaging needs a
supplied runtime, and no public server, directory endpoint, or relay.
