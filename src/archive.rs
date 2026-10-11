//! Mounts a zip archive (read fully into memory - nothing ever touches
//! disk) as a source for Lua's `require`, and provides the raw entry
//! lookup used to bootstrap the archive's own `init.lua`.

use crate::runtime::LuaResultExt;
use mlua::{Lua, Table, Value};
use std::cell::RefCell;
use std::io::{Cursor, Read};
use std::rc::Rc;

/// A zip archive whose bytes were slurped fully into memory. `zip::ZipArchive::new`
/// locates the central directory by scanning backward from the end of the
/// stream, so a shebang line (or anything else) glued onto the front is
/// transparently skipped - no manual offset handling is needed here.
pub type MemZip = zip::ZipArchive<Cursor<Vec<u8>>>;

/// Open `bytes` as a zip archive. Fails (cheaply) if `bytes` isn't one, so
/// callers can use this to distinguish an archive from a plain Lua script.
pub fn open(bytes: Vec<u8>) -> zip::result::ZipResult<MemZip> {
    zip::ZipArchive::new(Cursor::new(bytes))
}

/// Read one entry's fully decompressed bytes out of the archive.
pub fn read_entry(zip: &mut MemZip, name: &str) -> anyhow::Result<Vec<u8>> {
    let mut file = zip.by_name(name)?;
    let mut buf = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

/// Mount `zip` as a `require` source. A custom searcher is inserted into
/// `package.searchers` right after the `preload` searcher: it maps a
/// module name like `foo.bar` to `foo/bar.lua` or `foo/bar/init.lua`
/// inside the archive (the same two-shape rule Lua's own filesystem
/// searcher uses) and compiles it straight from the decompressed bytes.
/// The module source is never written to a temp file.
pub fn install_searcher(lua: &Lua, zip: MemZip) -> anyhow::Result<()> {
    let zip = Rc::new(RefCell::new(zip));

    let searcher = lua.create_function(move |lua, modname: String| -> mlua::Result<Value> {
        let base = modname.replace('.', "/");
        let candidates = [format!("{base}.lua"), format!("{base}/init.lua")];

        let mut zip = zip.borrow_mut();
        for candidate in &candidates {
            let mut file = match zip.by_name(candidate) {
                Ok(f) => f,
                Err(_) => continue,
            };
            let mut buf = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut buf).map_err(mlua::Error::external)?;
            drop(file);

            let func = lua
                .load(buf)
                .set_name(format!("@{candidate}"))
                .into_function()?;
            return Ok(Value::Function(func));
        }

        // Not found here: report it the way a normal searcher does, so
        // require()'s aggregated "module not found" error stays readable,
        // and let the next searcher (filesystem, C loader, ...) have a go.
        let msg: String = candidates
            .iter()
            .map(|c| format!("\n\tno file '{c}' in archive"))
            .collect();
        Ok(Value::String(lua.create_string(&msg)?))
    })
    .anyhow()?;

    let package: Table = lua.globals().get("package").anyhow()?;
    let searchers: Table = package.get("searchers").anyhow()?;
    let table: Table = lua.globals().get("table").anyhow()?;
    let table_insert: mlua::Function = table.get("insert").anyhow()?;
    // Position 2: after package.preload, before the filesystem/C loaders,
    // so archive contents win over anything of the same name on disk.
    table_insert.call::<()>((searchers, 2, searcher)).anyhow()?;

    Ok(())
}
