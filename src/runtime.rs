//! Small pieces of behavior shared by every run mode (REPL, directory,
//! zip archive, plain script): building the `arg` global the way the
//! stock `lua` interpreter does, wiring sibling files into `require`,
//! stripping a leading shebang line, and running a compiled chunk.

use mlua::{Lua, Table, Variadic};
use std::path::Path;

/// `mlua::Error` isn't guaranteed `Send + Sync` unless mlua's `send`
/// feature is enabled (which we deliberately don't, since our zip
/// searcher closure in `archive.rs` captures an `Rc`, not an `Arc`) - and
/// `anyhow::Error`'s blanket `From` impl requires exactly that. So at
/// every point an `mlua::Result` needs to become an `anyhow::Result`, we
/// convert it explicitly through `Display` instead of relying on `?` to
/// do it implicitly.
pub trait LuaResultExt<T> {
    fn anyhow(self) -> anyhow::Result<T>;
}

impl<T> LuaResultExt<T> for mlua::Result<T> {
    fn anyhow(self) -> anyhow::Result<T> {
        self.map_err(|e| anyhow::anyhow!("{e}"))
    }
}

/// If `src` begins with a shebang line (`#!...`), remove the shebang text
/// but keep exactly one newline in its place, so that any line number the
/// Lua compiler reports in an error still matches the original file.
/// This mirrors what the stock `lua.c` front end does before compiling.
pub fn strip_shebang(src: &[u8]) -> Vec<u8> {
    if src.first() != Some(&b'#') {
        return src.to_vec();
    }
    match src.iter().position(|&b| b == b'\n') {
        Some(nl) => {
            let mut out = Vec::with_capacity(src.len() - nl);
            out.push(b'\n');
            out.extend_from_slice(&src[nl + 1..]);
            out
        }
        // The whole file was a single unterminated shebang line.
        None => Vec::new(),
    }
}

/// Populate the Lua global `arg` table the same way `lua.c` does:
/// `arg[0]` is the script/archive itself, `arg[1..]` are the script's own
/// arguments, and `arg[-1]` is whatever ran the interpreter.
pub fn set_arg_table(lua: &Lua, exe: &str, script: &str, script_args: &[String]) -> mlua::Result<()> {
    let arg = lua.create_table()?;
    arg.set(-1, exe)?;
    if !script.is_empty() {
        arg.set(0, script)?;
    }
    for (i, a) in script_args.iter().enumerate() {
        arg.set((i + 1) as i64, a.clone())?;
    }
    lua.globals().set("arg", arg)?;
    Ok(())
}

/// Prepend `dir` to `package.path` and `package.cpath` in their native
/// template form, so `require` finds pure-Lua files and native
/// `.so`/`.dll`/`.dylib` modules that ship *next to* a script or archive
/// rather than embedded inside it.
pub fn add_search_dir(lua: &Lua, dir: &Path) -> mlua::Result<()> {
    let package: Table = lua.globals().get("package")?;
    let dir_str = dir.to_string_lossy();

    let lua_tmpl = format!("{dir_str}/?.lua;{dir_str}/?/init.lua;");
    let old_path: String = package.get("path").unwrap_or_default();
    package.set("path", format!("{lua_tmpl}{old_path}"))?;

    let ext = std::env::consts::DLL_EXTENSION; // "so" / "dll" / "dylib"
    let c_tmpl = format!("{dir_str}/?.{ext};{dir_str}/loadall.{ext};");
    let old_cpath: String = package.get("cpath").unwrap_or_default();
    package.set("cpath", format!("{c_tmpl}{old_cpath}"))?;

    Ok(())
}

/// Compile `src` under chunk name `chunk_name` and call it, passing
/// `script_args` as `...`, exactly like the stock interpreter passes the
/// script's own command-line arguments to the main chunk.
pub fn run_chunk(lua: &Lua, chunk_name: &str, src: &[u8], script_args: &[String]) -> anyhow::Result<()> {
    let func = lua.load(src).set_name(chunk_name).into_function().anyhow()?;

    let varargs = Variadic::from_iter(script_args.iter().cloned());
    func.call::<()>(varargs).anyhow()?;
    Ok(())
}
