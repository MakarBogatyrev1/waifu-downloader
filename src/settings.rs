use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub mode: String,
    pub category: String,
    pub save_mode: String,
    pub custom_save_folder: String,
    pub load_saved_mode: u8,
    pub use_custom_url: bool,
    pub custom_url: String,
    pub theme: Theme,
    pub accent: [u8; 3],
    pub system_accent: bool,
    pub prefetch: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: "SFW".into(),
            category: "waifu".into(),
            save_mode: "A".into(),
            custom_save_folder: String::new(),
            load_saved_mode: 2,
            use_custom_url: false,
            custom_url: String::new(),
            theme: Theme::System,
            accent: [159, 123, 255],
            system_accent: true,
            prefetch: true,
        }
    }
}

pub fn valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 100
        && tag
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

impl Settings {
    pub fn normalize(&mut self) {
        if self.mode != "SFW" && self.mode != "NSFW" {
            self.mode = "SFW".into();
        }
        if !valid_tag(&self.category) {
            self.category = "waifu".into();
        }
        if !["A", "B", "C"].contains(&self.save_mode.as_str()) {
            self.save_mode = "A".into();
        }
        if !(1..=3).contains(&self.load_saved_mode) {
            self.load_saved_mode = 2;
        }
    }

    pub fn load(base: &Path) -> (Self, Option<String>) {
        let current = base.join("settings-rust.json");
        let legacy = base.join("settings.json");
        let path = if current.exists() { current } else { legacy };
        if !path.exists() {
            return (Self::default(), None);
        }
        match fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|e| e.to_string())
            })
            .and_then(|value| {
                let mut settings: Self =
                    serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
                if !value.as_object().is_some_and(|v| v.contains_key("theme")) {
                    settings.theme = match value["appearance_mode"].as_str() {
                        Some("light") => Theme::Light,
                        Some("dark") => Theme::Dark,
                        _ => Theme::System,
                    };
                }
                settings.normalize();
                Ok(settings)
            }) {
            Ok(settings) => (settings, None),
            Err(e) => (
                Self::default(),
                Some(format!("Настройки не прочитаны: {e}")),
            ),
        }
    }

    pub fn save(&self, base: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let temp = base.join("settings-rust.json.tmp");
        fs::write(&temp, bytes).map_err(|e| e.to_string())?;
        fs::rename(temp, base.join("settings-rust.json")).map_err(|e| e.to_string())
    }

    pub fn save_folder(&self, base: &Path, mode: &str, tag: &str) -> PathBuf {
        match self.save_mode.as_str() {
            "B" => base.join("images"),
            "C" if !self.custom_save_folder.trim().is_empty() => {
                let folder = PathBuf::from(self.custom_save_folder.trim());
                if folder.is_absolute() {
                    folder
                } else {
                    base.join(folder)
                }
            }
            _ => base
                .join("images")
                .join(if mode == "NSFW" { "nsfw" } else { "sfw" })
                .join(if valid_tag(tag) { tag } else { "waifu" }),
        }
    }

    pub fn library_roots(&self, base: &Path) -> Vec<PathBuf> {
        let root = base.join("images");
        let mut roots = vec![match self.load_saved_mode {
            1 => root.join(self.mode.to_lowercase()).join(&self.category),
            2 => root.join(self.mode.to_lowercase()),
            _ => root.clone(),
        }];
        if self.save_mode == "B" {
            roots.push(root);
        }
        if self.save_mode == "C" && !self.custom_save_folder.trim().is_empty() {
            roots.push(self.save_folder(base, &self.mode, &self.category));
        }
        roots
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_legacy_values_are_normalized() {
        let mut s = Settings {
            mode: "bad".into(),
            category: "../x".into(),
            save_mode: "X".into(),
            load_saved_mode: 9,
            ..Settings::default()
        };
        s.normalize();
        assert_eq!(
            (&*s.mode, &*s.category, &*s.save_mode, s.load_saved_mode),
            ("SFW", "waifu", "A", 2)
        );
    }
    #[test]
    fn folder_modes_and_path_safety() {
        let base = Path::new("base");
        let mut s = Settings::default();
        assert_eq!(
            s.save_folder(base, "SFW", "maid"),
            base.join("images/sfw/maid")
        );
        assert_eq!(
            s.save_folder(base, "NSFW", "../escape"),
            base.join("images/nsfw/waifu")
        );
        s.save_mode = "B".into();
        assert_eq!(s.save_folder(base, "SFW", "maid"), base.join("images"));
        s.save_mode = "C".into();
        s.custom_save_folder = "custom".into();
        assert_eq!(s.save_folder(base, "SFW", "maid"), base.join("custom"));
    }
    #[test]
    fn migration_and_theme_roundtrip() {
        let base = std::env::temp_dir().join(format!(
            "waifu-settings-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("settings.json"),r#"{"appearance_mode":"light","category":"maid","save_mode":"C","custom_save_folder":"custom","use_custom_url":true,"custom_url":"https://custom.test"}"#).unwrap();
        let (mut s, error) = Settings::load(&base);
        assert!(error.is_none());
        assert_eq!(s.theme, Theme::Light);
        assert_eq!(s.category, "maid");
        assert_eq!(s.save_mode, "C");
        assert!(s.use_custom_url);
        s.theme = Theme::Dark;
        s.accent = [255, 100, 150];
        s.system_accent = false;
        s.save(&base).unwrap();
        s.theme = Theme::System;
        s.save(&base).unwrap();
        let (s, error) = Settings::load(&base);
        assert!(error.is_none());
        assert_eq!(s.theme, Theme::System);
        assert_eq!(s.accent, [255, 100, 150]);
        assert!(!s.system_accent);
        fs::remove_file(base.join("settings.json")).unwrap();
        fs::remove_file(base.join("settings-rust.json")).unwrap();
        fs::remove_dir(base).unwrap();
    }
    #[test]
    fn python_settings_shape_is_compatible() {
        let mut s: Settings = serde_json::from_str(
            r#"{"mode":"NSFW","category":"neko","save_mode":"B","thumb_size":96}"#,
        )
        .unwrap();
        s.normalize();
        assert_eq!(s.mode, "NSFW");
        assert_eq!(s.category, "neko");
        assert_eq!(s.theme, Theme::System);
        assert!(s.prefetch);
    }
}
