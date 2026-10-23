# Where each identity-scan hit on the Linux binary comes from

The scan is `identity-scan-linux-binary.txt` (`RESULT HIT 9`, no `build-record`). I read the bytes around each
offset in the unpacked bundle (`harness/hit_context_scan.py.txt`). Third-party addresses aren't repeated here.

| # | Class | Member | What is there | Whose |
|---|---|---|---|---|
| 1 | gmail | `carchive/base_library.zip/io.pyc` | the `io` module's author line (CPython stdlib) | third party: CPython |
| 2 | home-path | `carchive/base_library.zip/ntpath.pyc` | the `splitroot()` docstring example (`C:` / `Users` / a sample name) | third party: CPython |
| 3 | home-path | `carchive/libpython3.12.so.1.0` | the same `splitroot()` docstring, frozen into libpython | third party: CPython |
| 4 | gmail | `carchive/libpython3.12.so.1.0` | the same `io` author line, frozen into libpython | third party: CPython |
| 5 | gmail | `pyz/PYZ.pyz/bz2.pyc-body` | the `bz2` module's author line | third party: CPython |
| 6 | **home-path** | `pyz/PYZ.pyz/_sysconfigdata__linux_x86_64-linux-gnu.pyc-body` | **the build machine's home path**: uv's managed-Python install prefix, `<home>/.local/share/uv/python/cpython-3.12.13-linux-x86_64-gnu` (20 strings: `BINDIR`, `prefix`, `INCLUDEPY`, `DESTDIRS` …) | **the builder: this rig's WSL account here; `/home/runner/…` on CI** |
| 7 | gmail | `pyz/PYZ.pyz/httpx/_urls.pyc-body` | a docstring example address | third party: httpx |
| 8 | gmail | `pyz/PYZ.pyz/pygments/lexers/gdscript.pyc-body` | the GDScript lexer's author line | third party: pygments |
| 9 | gmail | `pyz/PYZ.pyz/rich/_spinners.pyc-body` | the MIT licence line of the spinners data | third party: rich (from cli-spinners) |

Hit 6 isn't third party. It is the path uv installed Python to on the machine that ran the build, written into
`_sysconfigdata` by uv when it installs the interpreter. The scanner's bundle-only `/home/<account>` check finds it.
The released xrpl-camp 1.5.0 Linux asset, built on CI, has the same 20 strings with
`/home/runner/work/_temp/uv-python-dir/cpython-3.12.15-linux-x86_64-gnu` (`sysconfigdata-home-paths.txt`). So a CI
build of this commit will carry `/home/runner/…` there, and the gate will report it. Removing `direct_url.json`
doesn't touch it.
