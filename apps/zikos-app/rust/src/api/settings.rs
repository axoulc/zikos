//! Settings read/write for the settings screen.

/// The user-configurable settings, mirrored for Dart. `default_format`
/// travels as a lowercase label (`"flac"` / `"wav"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsView {
    /// Matrix homeserver URL (`""` = unconfigured).
    pub homeserver_url: String,
    /// Audio blob server URL (`""` = unconfigured).
    pub blob_server_url: String,
    /// Default finalization format label.
    pub default_format: String,
}

/// The effective settings (defaults when never configured).
pub fn settings_get() -> SettingsView {
    crate::state::with_app(|app| to_view(app.settings()))
}

/// Validate and persist new settings. Fails with the first validation
/// error (e.g. `homeserver must be a valid https:// URL: …`) and keeps
/// the previous values.
pub fn settings_save(
    homeserver_url: String,
    blob_server_url: String,
    default_format: String,
) -> Result<SettingsView, String> {
    let format = parse_format(&default_format)?;
    let settings = crate::settings::Settings {
        homeserver_url,
        blob_server_url,
        default_format: format,
    };
    crate::state::with_app(|app| app.save_settings(settings)).map_err(|e| e.to_string())?;
    Ok(crate::state::with_app(|app| to_view(app.settings())))
}

/// Map domain settings to the Dart view (shared with `init`).
pub(crate) fn settings_view_of(settings: &crate::settings::Settings) -> SettingsView {
    SettingsView {
        homeserver_url: settings.homeserver_url.clone(),
        blob_server_url: settings.blob_server_url.clone(),
        default_format: crate::listing::format_label(settings.default_format),
    }
}

fn to_view(settings: &crate::settings::Settings) -> SettingsView {
    settings_view_of(settings)
}

fn parse_format(label: &str) -> Result<zikos_core::AudioFormat, String> {
    match label {
        "flac" => Ok(zikos_core::AudioFormat::Flac),
        "wav" => Ok(zikos_core::AudioFormat::Wav),
        other => Err(format!(
            "unsupported format {other:?} (use \"flac\" or \"wav\")"
        )),
    }
}
