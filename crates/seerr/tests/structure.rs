//! Structural guarantee: everything that changes Seerr's data lives in `src/write.rs`, where each
//! function takes a `RequestConfirmed`. Fails if a delete verb or the request endpoint shows up in
//! another file. (Sign-in POSTs are explicit user actions and are not requests.)
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
        // Ignore the test modules inside the file (they may mention endpoints in prose).
        let code = src.split("#[cfg(test)]").next().unwrap();
        for banned in [".delete(", ".put(", ".patch(", "\"/request"] {
            assert!(
                !code.contains(banned),
                "{name} contains `{banned}`: requests are created and cancelled only in write.rs, behind RequestConfirmed"
            );
        }
        // Posts: only the sign-in paths.
        for l in code.lines().filter(|l| l.contains(".post(")) {
            assert!(
                !l.contains("/request"),
                "{name}: a POST to the request endpoint belongs in write.rs"
            );
        }
    }
    let w = std::fs::read_to_string(format!("{dir}/write.rs")).unwrap();
    let code = w.split("#[cfg(test)]").next().unwrap();
    for f in ["create_request", "delete_request"] {
        let line = code
            .lines()
            .find(|l| l.contains(&format!("fn {f}")))
            .unwrap();
        assert!(
            line.contains("RequestConfirmed"),
            "{f} must take a RequestConfirmed"
        );
    }
}
