//! A small interactive REPL, standing in for the one that comes with
//! PUC Lua's `lua.c` (which mlua doesn't give us access to). Behavior it
//! deliberately matches:
//!   - a bare expression like `1 + 1` prints its value, like `lua -i` does
//!   - a statement that's syntactically incomplete (e.g. `if true then`)
//!     switches to a continuation prompt instead of erroring immediately
//!   - Ctrl-D exits, Ctrl-C abandons the current (possibly multi-line) entry

use mlua::{Lua, MultiValue};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::path::PathBuf;

fn history_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| PathBuf::from(h).join(".luex_history"))
}

pub fn run(lua: &Lua) -> anyhow::Result<()> {
    let mut rl = DefaultEditor::new()?;
    let hist = history_path();
    if let Some(p) = &hist {
        let _ = rl.load_history(p);
    }

    let version: String = lua
        .load("return _VERSION")
        .eval()
        .unwrap_or_else(|_| "Lua".to_string());
    println!("{version}  (luex REPL, Ctrl-D to exit)");

    'entries: loop {
        let mut buffer = String::new();
        let mut prompt = "> ";

        let func = 'read: loop {
            let line = match rl.readline(prompt) {
                Ok(line) => line,
                Err(ReadlineError::Interrupted) => continue 'entries,
                Err(ReadlineError::Eof) => {
                    println!();
                    if let Some(p) = &hist {
                        let _ = rl.save_history(p);
                    }
                    return Ok(());
                }
                Err(e) => return Err(e.into()),
            };

            if !buffer.is_empty() {
                buffer.push('\n');
            }
            buffer.push_str(&line);

            if buffer.trim().is_empty() {
                continue 'entries;
            }

            // Try it as an expression first, so `> 1 + 1` prints `2`
            // without the user having to type `return`.
            if let Ok(f) = lua.load(format!("return {buffer}")).into_function() {
                break 'read f;
            }

            match lua.load(buffer.as_str()).into_function() {
                Ok(f) => break 'read f,
                Err(mlua::Error::SyntaxError { incomplete_input: true, .. }) => {
                    prompt = ">> ";
                    continue 'read;
                }
                Err(e) => {
                    eprintln!("{e}");
                    let _ = rl.add_history_entry(buffer.as_str());
                    continue 'entries;
                }
            }
        };

        let _ = rl.add_history_entry(buffer.as_str());

        match func.call::<MultiValue>(()) {
            Ok(values) => print_results(lua, values),
            Err(e) => eprintln!("{e}"),
        }
    }
}

fn print_results(lua: &Lua, values: MultiValue) {
    if values.is_empty() {
        return;
    }
    let Ok(tostring) = lua.globals().get::<mlua::Function>("tostring") else {
        return;
    };
    let parts: Vec<String> = values
        .into_iter()
        .map(|v| tostring.call::<String>(v).unwrap_or_else(|_| "?".to_string()))
        .collect();
    println!("{}", parts.join("\t"));
}
