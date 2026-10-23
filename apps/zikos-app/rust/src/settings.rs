//! Settings exposed to the UI: homeserver, blob server URL, default
//! recording format. Persisted as JSON under the app's config directory;
//! validation lives here (Rust), never in Dart.

use std::fs;
use std::path::Path;
use zikos_core::AudioFormat;

/// File holding the persisted settings inside the config directory.
pub const SETTINGS_JSON: &str = "settings.json";

/// Everything the user can configure in the app.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    /// Matrix homeserver the app signs in against (`""` = unconfigured).
    pub homeserver_url: String,
    /// Audio blob server base URL (`""` = unconfigured).
    pub blob_server_url: String,
    /// Container finalized takes are written to. FLAC by default.
    pub default_format: AudioFormat,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            homeserver_url: String::new(),
            blob_server_url: String::new(),
            default_format: AudioFormat::Flac,
        }
    }
}

/// A settings value failed validation or persistence failed.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SettingsError {
    /// A URL field holds something that is not a valid `https://` URL.
    #[error("{field} must be a valid https:// URL: {reason}")]
    InvalidUrl {
        /// Which field failed (`"homeserver"` / `"blob server"`).
        field: &'static str,
        /// Human-readable reason shown next to the field.
        reason: String,
    },
    /// The chosen format cannot be selected (not implemented).
    #[error("format {0:?} is not supported")]
    UnsupportedFormat(AudioFormat),
    /// Reading or writing `settings.json` failed.
    #[error("settings i/o failed: {0}")]
    Io(String),
}

impl Settings {
    /// Validate field values: configured URLs must be `https://`, and the
    /// format must be one this build can finalize to (FLAC, WAV).
    /// Empty URLs are allowed (nothing configured yet).
    pub fn validate(&self) -> Result<(), SettingsError> {
        validate_url("homeserver", &self.homeserver_url)?;
        validate_url("blob server", &self.blob_server_url)?;
        match self.default_format {
            AudioFormat::Flac | AudioFormat::Wav => Ok(()),
            unsupported => Err(SettingsError::UnsupportedFormat(unsupported)),
        }
    }

    /// Load settings from `dir`, falling back to defaults when
    /// `settings.json` does not exist. A corrupt file is an error, never
    /// silently replaced.
    pub fn load(dir: &Path) -> Result<Self, SettingsError> {
        let bytes = match fs::read(dir.join(SETTINGS_JSON)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(SettingsError::Io(e.to_string())),
        };
        serde_json::from_slice(&bytes)
            .map_err(|e| SettingsError::Io(format!("invalid {SETTINGS_JSON}: {e}")))
    }

    /// Validate, then persist atomically (temp file + rename) as
    /// `settings.json` in `dir`.
    pub fn save(&self, dir: &Path) -> Result<(), SettingsError> {
        self.validate()?;
        fs::create_dir_all(dir).map_err(|e| SettingsError::Io(e.to_string()))?;
        let tmp = dir.join(format!("{SETTINGS_JSON}.tmp"));
        let bytes =
            serde_json::to_vec_pretty(self).map_err(|e| SettingsError::Io(e.to_string()))?;
        fs::write(&tmp, bytes).map_err(|e| SettingsError::Io(e.to_string()))?;
        fs::rename(&tmp, dir.join(SETTINGS_JSON)).map_err(|e| SettingsError::Io(e.to_string()))?;
        Ok(())
    }
}

/// An empty value means "not configured yet" and passes; anything else
/// must parse as a URL with the `https` scheme.
fn validate_url(field: &'static str, value: &str) -> Result<(), SettingsError> {
    if value.is_empty() {
        return Ok(());
    }
    match url::Url::parse(value) {
        Ok(parsed) if parsed.scheme() == "https" => Ok(()),
        Ok(_) => Err(SettingsError::InvalidUrl {
            field,
            reason: "URL scheme must be https".into(),
        }),
        Err(e) => Err(SettingsError::InvalidUrl {
            field,
            reason: e.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_flac_and_unconfigured() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::load(dir.path()).unwrap();
        assert_eq!(settings, Settings::default());
        assert_eq!(settings.default_format, AudioFormat::Flac);
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            homeserver_url: "https://matrix.example.org".into(),
            blob_server_url: "https://blob.example.org".into(),
            default_format: AudioFormat::Wav,
        };
        settings.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()).unwrap(), settings);
        assert!(
            dir.path().join(SETTINGS_JSON).is_file(),
            "settings persist as settings.json"
        );
    }

    #[test]
    fn rejects_malformed_and_plain_http_urls() {
        let settings = Settings {
            homeserver_url: "htp://matrix.example.org".into(),
            ..Settings::default()
        };
        assert_eq!(
            settings.validate(),
            Err(SettingsError::InvalidUrl {
                field: "homeserver",
                reason: "URL scheme must be https".into(),
            })
        );
        let settings = Settings {
            homeserver_url: "https://matrix.example.org".into(),
            blob_server_url: "http://insecure.example.org".into(),
            ..Settings::default()
        };
        assert_eq!(
            settings.validate(),
            Err(SettingsError::InvalidUrl {
                field: "blob server",
                reason: "URL scheme must be https".into(),
            })
        );
        // Not a URL at all.
        let settings = Settings {
            homeserver_url: "https://matrix.example.org".into(),
            blob_server_url: "not a url".into(),
            ..Settings::default()
        };
        assert!(matches!(
            settings.validate(),
            Err(SettingsError::InvalidUrl { .. })
        ));
    }

    #[test]
    fn empty_urls_mean_unconfigured_and_are_valid() {
        let settings = Settings::default();
        assert_eq!(settings.validate(), Ok(()));
    }

    #[test]
    fn rejects_unsupported_format() {
        let settings = Settings {
            default_format: AudioFormat::Opus,
            ..Settings::default()
        };
        assert_eq!(
            settings.validate(),
            Err(SettingsError::UnsupportedFormat(AudioFormat::Opus))
        );
    }

    #[test]
    fn save_validates_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            homeserver_url: "nope".into(),
            ..Settings::default()
        };
        assert!(settings.save(dir.path()).is_err());
        assert!(
            !dir.path().join(SETTINGS_JSON).exists(),
            "an invalid settings value must not touch the persisted file"
        );
    }

    #[test]
    fn corrupt_settings_file_is_an_error_not_defaults() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(SETTINGS_JSON), "{not json").unwrap();
        assert!(Settings::load(dir.path()).is_err());
    }
}
