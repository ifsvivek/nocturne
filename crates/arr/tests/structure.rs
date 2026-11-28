//! Structural guarantee: the only writes of this crate (`POST /movie`, `POST /series`) live in
//! `src/write.rs`, where each function takes an `ArrConfirmed`. Fails if a write verb shows up in
//! another file.
#[test]
fn writes_only_in_write_rs() {
    let dir = format!("{}/src", std::env::var("CARGO_MANIFEST_DIR").unwrap());
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name == "write.rs" {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        let code = src.split("#[cfg(test)]").next().unwrap();
        for banned in [".post(", ".delete(", ".put(", ".patch("] {
            assert!(
                !code.contains(banned),
                "{name} contains `{banned}`: adds happen only in write.rs, behind ArrConfirmed"
            );
        }
    }
    let w = std::fs::read_to_string(format!("{dir}/write.rs")).unwrap();
    let code = w.split("#[cfg(test)]").next().unwrap();
    for f in ["add_movie", "add_series"] {
        let line = code
            .lines()
            .find(|l| l.contains(&format!("fn {f}(")) && l.contains("async"))
            .unwrap();
        assert!(
            line.contains("ArrConfirmed"),
            "{f} must take an ArrConfirmed"
        );
    }
}
