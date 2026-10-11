# luex

A Lua runtime that runs a program straight out of a **zip archive** (with an
optional shebang line glued to the front and the executable bit set) the
same way `java -jar` runs a JAR or `pex`/`shiv` run a Python zipapp — or, if
you point it at a **directory**, runs `<dir>/init.lua` the way `python .`
runs `__main__.py`. Archives run entirely from memory; nothing is ever
extracted to a temp file.

Built on `mlua` (real, unmodified Lua 5.4, vendored and compiled from
source) and `rustyline` for the REPL.

## Build

```sh
cargo build --release
# binary at target/release/luex
```

`mlua`'s `vendored` feature compiles Lua's C sources itself, so you need a C
compiler (`cc`/`clang`/MSVC) available but nothing else — no system Lua
install required.

## Usage

```
luex                      # interactive REPL
luex script.lua args...   # run a plain Lua file (shebang stripped if present)
luex myapp args...        # run a zip archive (shebang optional) — myapp/init.lua
luex somedir/ args...     # run somedir/init.lua directly off disk
```

## Packaging an archive

```sh
mkdir myapp && cd myapp
cat > init.lua <<'EOF'
local name = ...
print("hello, " .. (name or "world"))
print("arg[0] =", arg[0])
EOF
cd ..
zip -r app.zip myapp/*        # or: (cd myapp && zip -r ../app.zip .)

# Glue a shebang onto the front and mark it executable:
{ printf '#!/usr/bin/env luex\n'; cat app.zip; } > myapp.run
chmod +x myapp.run
rm app.zip

./myapp.run world
# hello, world
# arg[0] =    ./myapp.run
```

This works because the zip format's index (the *End of Central Directory*
record) lives at the **end** of the file. `luex` (via the `zip` crate)
locates it by scanning backward from the end, so whatever bytes — a
shebang line, a whole other executable — sit in front of the archive data
are simply ignored when reading it, and the OS's shebang mechanism just
execs `luex myapp.run`, handing the *whole* file back to `luex` to
re-read and interpret itself.

### `require` inside an archive

Modules are resolved straight out of the archive's compressed bytes — a
custom `package.searchers` entry maps `require("foo.bar")` to
`foo/bar.lua` or `foo/bar/init.lua` inside the zip, exactly like Lua's
normal filesystem searcher maps dots to path separators. Nothing is ever
written to disk to satisfy a `require`.

### Native (`.so`/`.dll`/`.dylib`) modules

Per your requirement, native libraries are **not** embedded in the zip or
loaded from memory — they ship as ordinary sibling files next to the
archive (or next to a plain script, or inside a run-as-directory folder),
and `package.cpath` is set to look there. So:

```
myapp.run
myapp.run.deps/
  mylib.so         # Linux
  # mylib.dll on Windows, mylib.dylib on macOS — build per platform
```

Adjust the actual directory layout to taste; `luex` just prepends
`<dir-containing-the-archive>/?.<native-ext>` to `package.cpath`, where
`<native-ext>` is `so`, `dll`, or `dylib` depending on the platform
`luex` itself was built for. `require("mylib")` then finds it via Lua's
normal C-loader searcher, unmodified.

### Precompiled bytecode

Since you said these archives don't need to be cross-platform, you can
swap any `.lua` file in the zip for its precompiled bytecode (`luac -o
foo.lua foo.lua` on a matching build of `luex`'s Lua) for faster cold
starts — the loader doesn't care, `mlua`'s `load` accepts either text or
binary chunks transparently. Just remember bytecode is tied to the exact
Lua build (version, integer size, endianness) that produced it.

## REPL

`rustyline`-based, standing in for the REPL PUC Lua's own `lua.c` provides
(which isn't reachable through `mlua`). It mirrors the behavior people
expect from `lua -i`:

- a bare expression (`1 + 1`) prints its value without needing `return`
- an incomplete statement (`if true then`) switches to a `>>` continuation
  prompt instead of erroring immediately — this uses `mlua::Error`'s own
  `incomplete_input` flag, the same signal `mlua` exposes specifically for
  building REPLs, rather than pattern-matching error text
- Ctrl-C abandons the current (possibly multi-line) entry; Ctrl-D exits
- history persists to `~/.luex_history`

## Design notes / limitations

- **Detection, not a fixed magic byte**: a file is treated as an archive
  if `zip::ZipArchive::new` can parse it (i.e. it has a valid End-of-
  Central-Directory record somewhere), and as plain Lua source otherwise.
  Either way a leading `#!...` line is stripped before compiling.
- **Not a fork of PUC Lua.** `liblua` itself is the unmodified vendored C
  source `mlua` builds; all of the archive-aware behavior lives in this
  Rust host and a couple of Lua-side `package.searchers`/`package.cpath`
  entries, so upgrading the Lua version is a matter of bumping `mlua`'s
  version, not rebasing a patch set.
- **Cross-platform archives**: the zip itself (pure Lua source/bytecode)
  is portable; native modules are not embedded, by design, so a project
  targeting multiple OSes ships one native lib per platform alongside a
  single shared zip.
- Password-protected / encrypted zip entries aren't supported (not
  requested; would need the `zip` crate's `aes-crypto` feature).

## Project layout

```
Cargo.toml
src/
  main.rs      — mode dispatch: REPL / directory / archive / plain script
  archive.rs   — in-memory zip + the custom require() searcher
  runtime.rs   — shebang stripping, arg table, search-path wiring, chunk exec
  repl.rs      — rustyline-based interactive REPL
```

## A note on how this was written

This was authored and reviewed by hand against `mlua` 0.11's and the `zip`
crate's current documented APIs, but **it has not been compiled** — the
environment that produced it has no Rust toolchain or network access to
fetch crates. Run `cargo build --release` as your first step; if anything
doesn't line up with the exact `mlua`/`zip` versions Cargo resolves, the
error messages should point at very localized fixes (the code is
deliberately kept to a handful of small, single-purpose functions).
