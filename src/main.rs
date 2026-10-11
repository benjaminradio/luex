mod archive;
mod repl;
mod runtime;

use mlua::Lua;
use runtime::LuaResultExt;
use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("luapex: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let raw_args: Vec<String> = env::args().collect();
    let exe = raw_args.first().cloned().unwrap_or_else(|| "luapex".to_string());
    let lua = Lua::new();

    match raw_args.get(1) {
        None => {
            runtime::set_arg_table(&lua, &exe, "", &[]).anyhow()?;
            repl::run(&lua)?;
        }
        Some(target) => {
            let script_args = raw_args[2..].to_vec();
            let path = PathBuf::from(target);

            if path.is_dir() {
                run_directory(&lua, &exe, &path, target, &script_args)?;
            } else {
                run_file(&lua, &exe, &path, target, &script_args)?;
            }
        }
    }

    Ok(())
}

/// `luapex somedir` runs `somedir/init.lua`, exactly like `python somedir`
/// runs `somedir/__main__.py`. No archive is involved.
fn run_directory(
    lua: &Lua,
    exe: &str,
    dir: &Path,
    display_name: &str,
    script_args: &[String],
) -> anyhow::Result<()> {
    let entry = dir.join("init.lua");
    let src = std::fs::read(&entry).map_err(|e| anyhow::anyhow!("{}: {e}", entry.display()))?;

    runtime::add_search_dir(lua, dir).anyhow()?;
    runtime::set_arg_table(lua, exe, display_name, script_args).anyhow()?;
    let chunk_name = format!("@{}", entry.display());
    runtime::run_chunk(lua, &chunk_name, &src, script_args)
}

/// `luapex somefile` where `somefile` is either:
///   - a zip archive (optionally with a shebang glued to the front and the
///     executable bit set) containing a top-level `init.lua`, or
///   - a plain Lua source file (optionally with a shebang).
/// Either way it's run directly - the zip case never touches disk.
fn run_file(
    lua: &Lua,
    exe: &str,
    path: &Path,
    display_name: &str,
    script_args: &[String],
) -> anyhow::Result<()> {
    let bytes = std::fs::read(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;

    // Sibling native libraries (.so/.dll/.dylib) live next to the file
    // itself, whichever of the two modes below we end up in.
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    match archive::open(bytes.clone()) {
        Ok(mut zip) => {
            let entry_src = archive::read_entry(&mut zip, "init.lua").map_err(|_| {
                anyhow::anyhow!(
                    "{}: archive has no top-level 'init.lua' entry point",
                    path.display()
                )
            })?;

            archive::install_searcher(lua, zip)?;
            runtime::add_search_dir(lua, dir).anyhow()?;
            runtime::set_arg_table(lua, exe, display_name, script_args).anyhow()?;

            let src = runtime::strip_shebang(&entry_src);
            runtime::run_chunk(lua, "@init.lua", &src, script_args)
        }
        Err(_) => {
            // Not a zip: run it exactly like `lua script.lua` would,
            // stripping a shebang ourselves since we bypass lua.c.
            runtime::add_search_dir(lua, dir).anyhow()?;
            runtime::set_arg_table(lua, exe, display_name, script_args).anyhow()?;
            let src = runtime::strip_shebang(&bytes);
            let chunk_name = format!("@{display_name}");
            runtime::run_chunk(lua, &chunk_name, &src, script_args)
        }
    }
}
