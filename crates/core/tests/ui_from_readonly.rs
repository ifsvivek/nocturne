//! Using one server's UI over another is local and read-only: the code that picks and applies a
//! UI can reach neither the write crates nor an HTTP verb that writes, and the remote stylesheet
//! fetcher only ever issues GET. A source-level check like `crates/jellyfin/tests/readonly.rs`.
use std::path::Path;

fn read(rel: &str) -> String {
    std::fs::read_to_string(
        Path::new(std::env::var("CARGO_MANIFEST_DIR").unwrap().as_str()).join(rel),
    )
    .unwrap()
}

#[test]
fn picking_a_ui_cannot_reach_a_write_path() {
    for file in ["src/server_ui.rs"] {
        let src = read(file);
        // Only the code above the unit tests counts.
        let code = src.split("#[cfg(test)]").next().unwrap();
        for bad in [
            "nocturne_jellyfin_user",
            "nocturne_jellyfin_admin",
            "Confirmed",
            "reqwest",
            "bridge::",
            ".post(",
            ".put(",
            ".delete(",
            ".patch(",
            "std::fs::write",
        ] {
            assert!(!code.contains(bad), "{file} must not use `{bad}`");
        }
    }
}

#[test]
fn the_stylesheet_fetcher_only_reads() {
    let src = read("src/customize/import.rs");
    let code = src.split("#[cfg(test)]").next().unwrap();
    assert!(code.contains(".get("));
    for bad in [
        ".post(",
        ".put(",
        ".delete(",
        ".patch(",
        ".head(",
        "nocturne_jellyfin",
    ] {
        assert!(!code.contains(bad), "import.rs must not use `{bad}`");
    }
    // The only thing it writes is its own cache directory.
    assert_eq!(code.matches("std::fs::write").count(), 1);
}
