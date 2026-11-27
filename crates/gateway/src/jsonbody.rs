//! Case-insensitive JSON request bodies. ASP.NET binds property names without regard to case, and real clients rely on it
//! (jellyfin-apiclient-python posts `{"username": ..., "Pw": ...}`), so every body the gateway reads goes through here.
//! Only the top-level keys are folded to lower case: nested objects keep their keys (command `Arguments` are passed on as sent).
//! Deserialize into structs whose fields have lower-case names, or read a [`parse_value`] result with lower-case keys.
use serde::de::DeserializeOwned;
use serde_json::Value;

/// The top-level keys of an object, lower-cased; any other value is returned unchanged. A later duplicate (by case) wins.
pub fn lower_top(v: Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v))
                .collect(),
        ),
        other => other,
    }
}

/// Parse a request body into `T` (lower-case field names), whatever the case of the sender's keys.
pub fn parse<T: DeserializeOwned>(body: &[u8]) -> Option<T> {
    serde_json::from_value(lower_top(serde_json::from_slice(body).ok()?)).ok()
}

/// A body as a `Value` with lower-case top-level keys (for handlers that read fields by name).
pub fn parse_value(body: &[u8]) -> Option<Value> {
    serde_json::from_slice(body).ok().map(lower_top)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Deserialize, Debug, PartialEq)]
    struct Login {
        username: Option<String>,
        pw: Option<String>,
    }

    #[test]
    fn any_casing_binds_and_nested_keys_are_kept() {
        for body in [
            r#"{"Username":"a","Pw":"b"}"#,
            r#"{"username":"a","Pw":"b"}"#,
            r#"{"USERNAME":"a","pw":"b"}"#,
        ] {
            let l: Login = parse(body.as_bytes()).unwrap();
            assert_eq!(
                (l.username.as_deref(), l.pw.as_deref()),
                (Some("a"), Some("b")),
                "{body}"
            );
        }
        assert!(parse::<Login>(b"[]").is_none() && parse::<Login>(b"nope").is_none());
        let v = parse_value(br#"{"Name":"SetVolume","Arguments":{"Volume":"5"}}"#).unwrap();
        assert_eq!(
            v,
            json!({"name": "SetVolume", "arguments": {"Volume": "5"}})
        );
    }
}
