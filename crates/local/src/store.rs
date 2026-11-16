//! Persistence: the only module allowed to create, rename or remove files, and only the library's own JSON files in
//! its data directory (never anything under a media root; see `tests/readonly.rs`).

use crate::Error;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{fs, io::Write, path::Path};

pub const INDEX_VERSION: u32 = 1;

/// Write `bytes` to `path` via a temp file in the same directory, then rename over the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let io = |e| Error::Io(path.to_path_buf(), e);
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(io)?;
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("data")
    ));
    let mut f = fs::File::create(&tmp).map_err(io)?;
    f.write_all(bytes).map_err(io)?;
    f.sync_all().map_err(io)?;
    fs::rename(&tmp, path).map_err(io)
}

pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    write_atomic(path, &serde_json::to_vec_pretty(value)?)
}

/// `None` when the file does not exist.
pub fn load_value(path: &Path) -> Result<Option<Value>, Error> {
    match fs::read(path) {
        Ok(b) => Ok(Some(serde_json::from_slice(&b)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::Io(path.to_path_buf(), e)),
    }
}

pub fn load<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, Error> {
    load_value(path)?
        .map(|v| serde_json::from_value(v).map_err(Error::from))
        .transpose()
}

/// Bring an index document of any known older version to `INDEX_VERSION`.
/// v0 (the pre-release layout) had only `items`: no version, roots, shows or id counter.
pub fn migrate(mut v: Value) -> Result<Value, Error> {
    let version = v.get("version").and_then(Value::as_u64).unwrap_or(0) as u32;
    if version > INDEX_VERSION {
        return Err(Error::NewerSchema(version));
    }
    if version == 0 {
        let next = v["items"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|i| i["id"].as_u64())
                    .max()
                    .map_or(1, |m| m + 1)
            })
            .unwrap_or(1);
        let o = v
            .as_object_mut()
            .ok_or(Error::Corrupt("index is not an object"))?;
        o.entry("items").or_insert(json!([]));
        o.entry("roots").or_insert(json!([]));
        o.entry("shows").or_insert(json!([]));
        o.insert("next_id".into(), json!(next));
        o.insert("version".into(), json!(1));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_v0_to_v1() {
        let old = json!({"items": [{"id": 4}, {"id": 9}]});
        let new = migrate(old).unwrap();
        assert_eq!(new["version"], 1);
        assert_eq!(new["next_id"], 10);
        assert_eq!(new["roots"], json!([]));
        assert_eq!(new["items"].as_array().unwrap().len(), 2);
        // idempotent
        assert_eq!(migrate(new.clone()).unwrap(), new);
    }

    #[test]
    fn refuses_newer_schema() {
        assert!(matches!(
            migrate(json!({"version": 99})),
            Err(Error::NewerSchema(99))
        ));
    }
}
