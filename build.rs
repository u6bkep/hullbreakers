//! Embeds every ship design and module (`assets/ships/**/*.ron`) in the
//! binary, so the simulation has them without loading assets (see
//! `src/sim/design.rs`).

use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/ships");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    files.sort();
    let mut out = String::from("/// Every `.ron` under `assets/ships`: (path without `.ron`, contents).\npub const SHIP_FILES: &[(&str, &str)] = &[\n");
    for (name, path) in files {
        out += &format!("    ({name:?}, include_str!({:?})),\n", path.display().to_string());
    }
    out += "];\n";
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("ship_files.rs");
    std::fs::write(dest, out).unwrap();
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
