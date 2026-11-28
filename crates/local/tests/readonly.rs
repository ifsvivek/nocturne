//! Structural guarantee: the library never modifies, moves or deletes anything under a media root.
//! Only `store.rs` (the library's own JSON files, written atomically in the data directory) may create, rename or
//! remove files; every other source file must be free of mutating filesystem calls.

use std::path::Path;

const MUTATING: [&str; 14] = [
    "OpenOptions",
    "File::create",
    "create_new(",
    "fs::write(",
    "remove_file(",
    "remove_dir",
    "fs::rename(",
    "fs::copy(",
    "create_dir",
    "set_permissions(",
    "hard_link(",
    "symlink(",
    "set_modified(",
    "set_times(",
];

fn sources() -> Vec<(String, String)> {
    std::fs::read_dir(Path::new(std::env::var("CARGO_MANIFEST_DIR").unwrap().as_str()).join("src"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&p).unwrap(),
            )
        })
        .collect()
}

/// Source text without `#[cfg(test)]` modules (tests may build their own fixtures).
fn non_test(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap()
}

#[test]
fn only_store_may_mutate_the_filesystem() {
    for (name, src) in sources() {
        if name == "store.rs" {
            continue;
        }
        for bad in MUTATING {
            assert!(
                !non_test(&src).contains(bad),
                "src/{name} contains `{bad}`: nocturne-local must treat media roots as strictly read-only. \
                 Writes belong only in store.rs, and only for the library's own JSON files."
            );
        }
    }
}

#[test]
fn store_never_takes_a_media_path() {
    // store.rs is generic over paths, so check what feeds it: the data-directory files are the only callers.
    let lib = sources()
        .into_iter()
        .find(|(n, _)| n == "lib.rs")
        .unwrap()
        .1;
    for call in lib
        .lines()
        .filter(|l| l.contains("store::save(") || l.contains("store::write_atomic("))
    {
        assert!(
            call.contains("self.dir.join(\""),
            "store write not scoped to the data dir: {call}"
        );
    }
}

#[test]
fn process_spawning_is_limited_to_ffprobe() {
    for (name, src) in sources() {
        if name != "probe.rs" {
            assert!(
                !non_test(&src).contains("Command::new"),
                "src/{name} spawns a process"
            );
        }
    }
    let probe = sources()
        .into_iter()
        .find(|(n, _)| n == "probe.rs")
        .unwrap()
        .1;
    assert_eq!(
        non_test(&probe)
            .matches("Command::new(\"ffprobe\")")
            .count(),
        2
    );
    assert_eq!(non_test(&probe).matches("Command::new").count(), 2);
}
