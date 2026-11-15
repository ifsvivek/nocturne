//! Structural guarantee: the library never issues write requests.
//! Fails if any non-GET HTTP verb helper or `Method::` override appears in src/.

#[test]
fn no_write_verbs_in_src() {
    let banned = ["post", "put", "delete", "patch"].map(|v| format!(".{v}("));
    let method = ["Method", "::"].concat();
    let request = [".req", "uest("].concat();
    for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src")).unwrap() {
        let path = entry.unwrap().path();
        let src = std::fs::read_to_string(&path).unwrap();
        for b in banned.iter().chain([&method, &request]) {
            assert!(
                !src.contains(b.as_str()),
                "{} contains `{b}`: nocturne-jellyfin is intentionally READ-ONLY (GET only) because \
                 it fronts the user's personal media. Do not add write calls here. If one is truly \
                 needed, first record an explicit decision in STATUS.md, then change this guard.",
                path.display()
            );
        }
    }
}
