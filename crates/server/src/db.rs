//! The server's state store: one redb file (`$XDG_DATA_HOME/nocturne/server/state.redb`) shared by every module. Every record is JSON under a `u64` id in its own table (a module adds a table by implementing [`Rec`] for its type, with the [`rec!`] macro; tables appear on first write); `meta` holds the schema version, id counters and job runs. No secrets here (keys and passwords' plaintext never; password hashes and token hashes only).
use nocturne_arr_types::IndexerDef;
use nocturne_core::server::{
    BlockEntry, Grab, HistoryEntry, Library, MediaRequest, Profile, Title, Unit,
};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition, TableError, TableHandle};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::Path;

pub type Res<T> = Result<T, String>;

/// Bump with a migration in [`Db::open`] when a stored shape changes incompatibly (new optional fields need none: serde defaults).
pub const SCHEMA: u64 = 1;

const META: TableDefinition<&str, u64> = TableDefinition::new("meta");

/// A record type with its own table.
pub trait Rec: Serialize + DeserializeOwned {
    const TABLE: TableDefinition<'static, u64, &'static [u8]>;
    fn id(&self) -> u64;
    fn set_id(&mut self, id: u64);
}

/// `rec!(Type, "table")` for a type with a `pub id: u64` field.
#[macro_export]
macro_rules! rec {
    ($t:ty, $name:literal) => {
        impl $crate::db::Rec for $t {
            const TABLE: redb::TableDefinition<'static, u64, &'static [u8]> =
                redb::TableDefinition::new($name);
            fn id(&self) -> u64 {
                self.id
            }
            fn set_id(&mut self, id: u64) {
                self.id = id;
            }
        }
    };
}
rec!(Library, "libraries");
rec!(Profile, "profiles");
rec!(IndexerDef, "indexers");
rec!(Title, "titles");
rec!(Unit, "units");
rec!(Grab, "grabs");
rec!(HistoryEntry, "history");
rec!(BlockEntry, "blocklist");
rec!(MediaRequest, "requests");

fn e(x: impl std::fmt::Display) -> String {
    format!("database: {x}")
}

fn missing_is_none<T>(r: Result<T, TableError>) -> Res<Option<T>> {
    match r {
        Ok(t) => Ok(Some(t)),
        Err(TableError::TableDoesNotExist(_)) => Ok(None),
        Err(x) => Err(e(x)),
    }
}

pub struct Db(Database);

impl Db {
    /// Open or create; refuses a file from a newer version.
    pub fn open(path: &Path) -> Res<Self> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(e)?;
        }
        let db = Database::create(path).map_err(e)?;
        // Password and token hashes: only the owner reads the file.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(e)?;
        }
        let tx = db.begin_write().map_err(e)?;
        {
            let mut meta = tx.open_table(META).map_err(e)?;
            let v = meta.get("schema").map_err(e)?.map(|g| g.value());
            match v {
                None => {
                    meta.insert("schema", SCHEMA).map_err(e)?;
                }
                Some(v) if v > SCHEMA => {
                    return Err(format!(
                        "the server database is from a newer NOCTURNE (schema {v}); update NOCTURNE"
                    ));
                }
                Some(_) => {}
            }
        }
        tx.commit().map_err(e)?;
        Ok(Self(db))
    }

    /// Insert or replace; id 0 gets the next free id (never reused). Returns the id.
    pub fn put<T: Rec>(&self, r: &mut T) -> Res<u64> {
        self.put_all(std::slice::from_mut(r)).map(|ids| ids[0])
    }

    /// [`Db::put`] for several records in one transaction.
    pub fn put_all<T: Rec>(&self, rs: &mut [T]) -> Res<Vec<u64>> {
        let tx = self.0.begin_write().map_err(e)?;
        let mut ids = Vec::with_capacity(rs.len());
        {
            let mut meta = tx.open_table(META).map_err(e)?;
            let mut t = tx.open_table(T::TABLE).map_err(e)?;
            let key = format!("next:{}", T::TABLE.name());
            for r in rs.iter_mut() {
                if r.id() == 0 {
                    let next = meta.get(key.as_str()).map_err(e)?.map_or(1, |g| g.value());
                    meta.insert(key.as_str(), next + 1).map_err(e)?;
                    r.set_id(next);
                }
                let bytes = serde_json::to_vec(r).map_err(e)?;
                t.insert(r.id(), bytes.as_slice()).map_err(e)?;
                ids.push(r.id());
            }
        }
        tx.commit().map_err(e)?;
        Ok(ids)
    }

    pub fn get<T: Rec>(&self, id: u64) -> Res<Option<T>> {
        let tx = self.0.begin_read().map_err(e)?;
        let Some(t) = missing_is_none(tx.open_table(T::TABLE))? else {
            return Ok(None);
        };
        match t.get(id).map_err(e)? {
            Some(g) => serde_json::from_slice(g.value()).map(Some).map_err(e),
            None => Ok(None),
        }
    }

    /// Every record, by id. Unreadable records are skipped (a downgrade must not brick the daemon).
    pub fn all<T: Rec>(&self) -> Res<Vec<T>> {
        let tx = self.0.begin_read().map_err(e)?;
        let Some(t) = missing_is_none(tx.open_table(T::TABLE))? else {
            return Ok(vec![]);
        };
        let mut out = Vec::new();
        for row in t.iter().map_err(e)? {
            let (_, v) = row.map_err(e)?;
            if let Ok(r) = serde_json::from_slice(v.value()) {
                out.push(r);
            }
        }
        Ok(out)
    }

    pub fn delete<T: Rec>(&self, ids: &[u64]) -> Res<()> {
        let tx = self.0.begin_write().map_err(e)?;
        {
            let mut t = tx.open_table(T::TABLE).map_err(e)?;
            for id in ids {
                t.remove(*id).map_err(e)?;
            }
        }
        tx.commit().map_err(e)
    }

    pub fn meta(&self, key: &str) -> Res<Option<u64>> {
        let tx = self.0.begin_read().map_err(e)?;
        let t = tx.open_table(META).map_err(e)?;
        Ok(t.get(key).map_err(e)?.map(|g| g.value()))
    }

    pub fn set_meta(&self, key: &str, v: u64) -> Res<()> {
        let tx = self.0.begin_write().map_err(e)?;
        tx.open_table(META).map_err(e)?.insert(key, v).map_err(e)?;
        tx.commit().map_err(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nocturne_arr_types::MediaKind;

    #[test]
    fn ids_are_assigned_never_reused_and_survive_reopen() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a/state.redb");
        let db = Db::open(&p).unwrap();
        let mut a = Profile::standard(MediaKind::Movie);
        let mut b = Profile::standard(MediaKind::Book);
        assert_eq!((db.put(&mut a).unwrap(), db.put(&mut b).unwrap()), (1, 2));
        db.delete::<Profile>(&[2]).unwrap();
        let mut c = Profile::standard(MediaKind::Music);
        assert_eq!(db.put(&mut c).unwrap(), 3);
        a.name = "Renamed".into();
        db.put(&mut a).unwrap();
        db.set_meta("job:rss_sync", 42).unwrap();
        drop(db);
        let db = Db::open(&p).unwrap();
        let all = db.all::<Profile>().unwrap();
        assert_eq!(all.iter().map(|p| p.id).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(db.get::<Profile>(1).unwrap().unwrap().name, "Renamed");
        assert_eq!(db.get::<Profile>(2).unwrap(), None);
        assert_eq!(db.meta("job:rss_sync").unwrap(), Some(42));
        assert_eq!(db.meta("schema").unwrap(), Some(SCHEMA));
    }

    #[test]
    fn refuses_a_newer_schema() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("state.redb");
        Db::open(&p)
            .unwrap()
            .set_meta("schema", SCHEMA + 1)
            .unwrap();
        assert!(Db::open(&p).err().unwrap().contains("newer"));
    }
}
