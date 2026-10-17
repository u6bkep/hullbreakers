//! Embeds every ship design and module (`assets/ships/**/*.ron`) in the
//! binary, so the simulation has them without loading assets (see
//! `src/sim/design.rs`), and every named ambience (`assets/ambience/*.ron`,
//! see `src/sim/ambience.rs`), so a host without the assets can still set
//! one.

use std::path::Path;

fn main() {
    // The manifest's directory as cargo gives it now, not as it was when
    // this script was compiled (`env!`): a build script is reused across
    // copies of the tree with the same source, and one compiled in a copy
    // since deleted walked nothing and embedded no designs at all.
    let assets = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets");
    embed(&assets.join("ships"), "SHIP_FILES", "ship_files.rs", "ship designs");
    embed(&assets.join("ambience"), "AMBIENCE_FILES", "ambience_files.rs", "ambiences");
}

/// Writes `OUT_DIR/<out>`: a `const <name>` of every `.ron` under `root`, as
/// (path without `.ron`, contents).
fn embed(root: &Path, name: &str, out: &str, what: &str) {
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    walk(root, root, &mut files);
    assert!(!files.is_empty(), "no {what} under {}", root.display());
    files.sort();
    let mut text = format!(
        "/// Every `.ron` under `assets/{}`: (path without `.ron`, contents).\npub const {name}: &[(&str, &str)] = &[\n",
        root.file_name().unwrap().to_string_lossy()
    );
    for (name, path) in files {
        text += &format!("    ({name:?}, include_str!({:?})),\n", path.display().to_string());
    }
    text += "];\n";
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join(out);
    std::fs::write(dest, text).unwrap();
}

fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, std::path::PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Directories are watched too, for files added to them.
            println!("cargo:rerun-if-changed={}", path.display());
            walk(&path, root, out);
        } else if path.extension().is_some_and(|e| e == "ron") {
            println!("cargo:rerun-if-changed={}", path.display());
            let name = path.strip_prefix(root).unwrap().with_extension("");
            out.push((name.to_string_lossy().replace('\\', "/"), path));
        }
    }
}
