//! Typed reads of what makes a Jellyfin server look like itself: the user's display preferences
//! (`CustomPrefs`, where jellyfin-web keeps the home section order), the user's configuration
//! (library order and exclusions) and the public branding. GET only; the matching write lives in
//! `nocturne-jellyfin-user`.
use crate::{Client, Result};
use serde::Deserialize;
use std::collections::HashMap;

/// `GET /DisplayPreferences/usersettings?client=emby`. Only `CustomPrefs` matters; its values are
/// always strings (or null).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct DisplayPrefs {
    pub custom_prefs: HashMap<String, Option<String>>,
}

impl DisplayPrefs {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.custom_prefs.get(key)?.as_deref()
    }
}

/// The `Configuration` block of `GET /Users/{id}`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct UserConfig {
    pub ordered_views: Vec<String>,
    pub my_media_excludes: Vec<String>,
    pub latest_items_excludes: Vec<String>,
    pub hide_played_in_latest: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct UserDoc {
    configuration: UserConfig,
}

/// `GET /Branding/Configuration` (public). Absent fields are empty.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Branding {
    pub login_disclaimer: Option<String>,
    pub custom_css: Option<String>,
}

impl Client {
    pub async fn display_prefs(&self, user_id: &str) -> Result<DisplayPrefs> {
        self.get(
            &["DisplayPreferences", "usersettings"],
            &[("userId", user_id.to_string()), ("client", "emby".into())],
        )
        .await
    }

    pub async fn user_config(&self, user_id: &str) -> Result<UserConfig> {
        let d: UserDoc = self.get(&["Users", user_id], &[]).await?;
        Ok(d.configuration)
    }

    pub async fn branding(&self) -> Result<Branding> {
        self.get(&["Branding", "Configuration"], &[]).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_shapes_and_tolerates_gaps() {
        let p: DisplayPrefs = serde_json::from_str(
            r#"{"Id":"x","CustomPrefs":{"homesection0":"resume","tvhome":null,"skipBackLength":"10000"}}"#,
        )
        .unwrap();
        assert_eq!(p.get("homesection0"), Some("resume"));
        assert_eq!(p.get("tvhome"), None);
        assert_eq!(p.get("nope"), None);
        let none: DisplayPrefs = serde_json::from_str("{}").unwrap();
        assert!(none.custom_prefs.is_empty());
        let u: UserDoc = serde_json::from_str(
            r#"{"Name":"a","Configuration":{"OrderedViews":["b","a"],"MyMediaExcludes":["c"],"HidePlayedInLatest":true}}"#,
        )
        .unwrap();
        assert_eq!(u.configuration.ordered_views, ["b", "a"]);
        assert!(u.configuration.hide_played_in_latest);
        let b: Branding = serde_json::from_str(r#"{"SplashscreenEnabled":false}"#).unwrap();
        assert!(b.custom_css.is_none() && b.login_disclaimer.is_none());
    }
}
