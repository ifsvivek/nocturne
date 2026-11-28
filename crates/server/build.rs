//! Embeds `web/**` (the dashboard: plain files, no build step) as a table of `(path, bytes)` in `$OUT_DIR/web_files.rs`.
use std::path::Path;

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(root, &p, out);
        } else if let (Ok(rel), Some(abs)) = (p.strip_prefix(root), p.to_str()) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            out.push(format!("({rel:?}, include_bytes!({abs:?}) as &[u8]),"));
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("web");
    println!("cargo:rerun-if-changed=web");
    let mut rows = Vec::new();
    walk(&root, &root, &mut rows);
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("web_files.rs");
    std::fs::write(
        out,
        format!(
            "pub static FILES: &[(&str, &[u8])] = &[\n{}\n];\n",
            rows.join("\n")
        ),
    )
    .unwrap();
}
