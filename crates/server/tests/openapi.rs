//! Every route the server registers (core and every module of this build) is in docs/api/openapi.yaml.
use nocturne_core::server::ServerConfig;
use nocturne_server::module::Registry;
use nocturne_server::state::{Paths, Secrets, Shared};
use std::collections::HashSet;

/// `(method, path)` pairs of the document: paths at two spaces, methods at four (the layout the file keeps).
fn documented() -> HashSet<(String, String)> {
    let text = std::fs::read_to_string(format!(
        "{}/../../docs/api/openapi.yaml",
        std::env::var("CARGO_MANIFEST_DIR").unwrap()
    ))
    .unwrap();
    let mut out = HashSet::new();
    let mut path = None;
    for line in text.lines() {
        if let Some(p) = line.strip_prefix("  /").and_then(|l| l.strip_suffix(':')) {
            path = Some(format!("/{p}"));
        } else if !line.starts_with("    ") && !line.trim().is_empty() && !line.starts_with("  ") {
            path = None;
        } else if let (Some(p), Some(m)) = (
            &path,
            line.strip_prefix("    ").and_then(|l| l.strip_suffix(':')),
        ) && ["get", "post", "put", "delete", "patch"].contains(&m)
        {
            out.insert((m.to_string(), p.clone()));
        }
    }
    out
}

#[test]
fn every_registered_route_is_documented() {
    let d = tempfile::tempdir().unwrap();
    let sh = Shared::new(
        Paths::under(d.path()),
        ServerConfig::default(),
        Secrets::memory(),
        Registry::new(nocturne_server::modules::all()),
    )
    .unwrap();
    let (_, routes) = nocturne_server::api::build(&sh);
    let doc = documented();
    assert!(doc.len() > 20, "the parser found the paths");
    let missing: Vec<_> = routes
        .iter()
        .filter(|(m, p)| !doc.contains(&(m.to_string(), p.clone())))
        .collect();
    assert!(
        missing.is_empty(),
        "not in docs/api/openapi.yaml: {missing:?}"
    );
}
