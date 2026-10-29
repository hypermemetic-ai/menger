//! `ui-compiler <file.svelte> -o <dir>`: writes `<dir>/<name>.json` (the view
//! tree) and `<dir>/<name>.rhai` (the handlers), `<name>` the file's stem.
//! A refusal is printed `file:line: message` and exits 1; a usage error 2.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (file, dir) = match args.as_slice() {
        [f, o, d] if o == "-o" => (PathBuf::from(f), PathBuf::from(d)),
        _ => {
            eprintln!("usage: ui-compiler <file.svelte> -o <dir>");
            return ExitCode::from(2);
        }
    };
    let src = match std::fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}: {e}", file.display());
            return ExitCode::from(2);
        }
    };
    let name = file.file_stem().and_then(|s| s.to_str()).unwrap_or("component").to_string();
    match menger_ui_compiler::compile(&src) {
        Ok(c) => {
            let write = std::fs::create_dir_all(&dir)
                .and_then(|_| std::fs::write(dir.join(format!("{name}.json")), &c.tree))
                .and_then(|_| std::fs::write(dir.join(format!("{name}.rhai")), &c.handlers));
            match write {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{}: {e}", dir.display());
                    ExitCode::from(2)
                }
            }
        }
        Err(r) => {
            eprintln!("{}:{}: {}", file.display(), r.line, r.message);
            ExitCode::from(1)
        }
    }
}
