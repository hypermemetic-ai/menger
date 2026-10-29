//! UI-5: the binary `ui-compiler <file.svelte> -o <dir>` writes the goldens.

use std::path::Path;

#[test]
fn the_binary_writes_the_goldens() {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("ui-compiler-bin");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_ui-compiler"))
        .arg(golden.join("ticket-row.svelte"))
        .arg("-o")
        .arg(&out)
        .status()
        .expect("the binary runs");
    assert!(status.success(), "ui-compiler exited {status}");
    for ext in ["json", "rhai"] {
        let written = std::fs::read(out.join(format!("ticket-row.{ext}"))).expect("written");
        let want = std::fs::read(golden.join(format!("ticket-row.{ext}"))).expect("the golden");
        assert!(written == want, "ticket-row.{ext} differs from the golden");
    }
}
