# Where each hit in `11-identity-scan-unpacked.txt` comes from

`harness/hit_context.py.txt` printed the bytes around every hit. The addresses and paths are described here
instead of copied, so this folder passes the identity scan. None of the 28 hits is this rig's account, paths or
email. A direct search of the unpacked tree for the rig's username, its worktree and run folders, and the
author's noreply id found 0 files.

| Class | Count | Where | What the bytes are |
|---|---:|---|---|
| home-path | 18 | `python312.dll` and 17 extension modules (`_asyncio`, `_bz2`, `_ctypes`, `_decimal`, `_hashlib`, `_lzma`, `_multiprocessing`, `_overlapped`, `_queue`, `_socket`, `_ssl`, `_uuid`, `_wmi`, `pyexpat`, `select`, `unicodedata`, `winsound`) | The PDB path of the CPython build machine: a `python-build-*` temp folder under its Windows account `Administrator`. It comes with uv's managed CPython (python-build-standalone). |
| home-path | 2 | `libssl-3-x64.dll`, `libcrypto-3-x64.dll` | The same build machine's `openssl-build-*` temp folder (once as the 8.3 short name `ADMINI~1`). |
| home-path | 1 | `libffi-8.dll` | The same build machine's `libffi-build-*` temp folder. |
| home-path | 1 | `base_library.zip/ntpath.pyc` | A CPython docstring example of `splitroot` on a Windows profile path for a made-up user, Barney. |
| gmail | 2 | `base_library.zip/io.pyc`, `python312.dll` | The `io` module's author line (a CPython contributor). |
| gmail | 1 | `bz2.pyc` | The `bz2` module's author line (a CPython contributor). |
| gmail | 1 | `rich/_spinners.pyc` | The MIT licence header of the spinner data rich vendors (the cli-spinners author). |
| gmail | 1 | `pygments/lexers/gdscript.pyc` | The lexer's author credit. |
| gmail | 1 | `httpx/_urls.pyc` | A docstring example: `username="jo@…"` with `password="a secret"`. |
