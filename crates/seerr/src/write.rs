//! Everything that changes anything on the Seerr server lives in this one file, and every function
//! here takes a [`RequestConfirmed`]. `tests/structure.rs` fails if a write verb appears elsewhere.
use crate::model::*;
use crate::{Error, RequestConfirmed, Result, SeerrClient};
use serde_json::{Map, Value, json};

/// The JSON body of `POST /request`: only the plain fields. A show always carries `seasons`
/// (Seerr answers 500 without it).
pub(crate) fn body(r: &NewRequest) -> Value {
    let mut m = Map::new();
    m.insert("mediaType".into(), json!(r.kind.as_str()));
    m.insert("mediaId".into(), json!(r.tmdb_id));
    if r.kind == Kind::Tv {
        let s = match &r.seasons {
            Seasons::All => json!(SeasonsBody::All("all")),
            Seasons::Some(v) => json!(SeasonsBody::List(v)),
        };
        m.insert("seasons".into(), s);
    }
    Value::Object(m)
}

impl SeerrClient {
    /// Ask Seerr for a movie or show. A normal user's request is pending approval; an administrator's
    /// is approved at once.
    pub async fn create_request(&self, r: &NewRequest, _c: RequestConfirmed) -> Result<Request> {
        if self.is_api_key() {
            return Err(Error::ReadOnly);
        }
        let path = "/request";
        let rb = self.authed(self.http.post(self.url(path)).json(&body(r)));
        self.send(rb, path, false)
            .await?
            .json()
            .await
            .map_err(|e| Error::Http(e.without_url()))
    }

    /// Cancel one of your own pending requests.
    pub async fn delete_request(&self, id: u64, _c: RequestConfirmed) -> Result<()> {
        if self.is_api_key() {
            return Err(Error::ReadOnly);
        }
        let path = format!("/request/{id}");
        let rb = self.authed(self.http.delete(self.url(&path)));
        self.send(rb, &path, false).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_carry_only_the_plain_fields() {
        let m = NewRequest {
            kind: Kind::Movie,
            tmdb_id: 348,
            seasons: Seasons::All,
        };
        assert_eq!(body(&m), json!({"mediaType": "movie", "mediaId": 348}));
        let t = NewRequest {
            kind: Kind::Tv,
            tmdb_id: 1396,
            seasons: Seasons::Some(vec![1, 2]),
        };
        assert_eq!(
            body(&t),
            json!({"mediaType": "tv", "mediaId": 1396, "seasons": [1, 2]})
        );
        let a = NewRequest {
            kind: Kind::Tv,
            tmdb_id: 1,
            seasons: Seasons::All,
        };
        assert_eq!(
            body(&a),
            json!({"mediaType": "tv", "mediaId": 1, "seasons": "all"})
        );
    }
}
