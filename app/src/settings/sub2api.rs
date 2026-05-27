//! sub2api desktop integration settings.

use settings::{define_settings_group, RespectUserSyncSetting, SupportedPlatforms, SyncToCloud};

pub const DEFAULT_SERVER_URL: &str = "https://www.axaihub.com";
const LEGACY_DEFAULT_SERVER_URL: &str = "http://www.axaihub.com";

define_settings_group!(Sub2ApiSettings, settings: [
    server_url: Sub2ApiServerUrl {
        type: String,
        default: DEFAULT_SERVER_URL.to_owned(),
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::No),
        private: false,
        toml_path: "sub2api.server_url",
        description: "sub2api server URL used for account login and API key bootstrap.",
    }
]);

pub fn normalize_server_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        DEFAULT_SERVER_URL.to_owned()
    } else if trimmed == LEGACY_DEFAULT_SERVER_URL {
        DEFAULT_SERVER_URL.to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_server_url_uses_https_origin() {
        assert_eq!(DEFAULT_SERVER_URL, "https://www.axaihub.com");
    }

    #[test]
    fn normalize_server_url_uses_default_for_empty_values() {
        assert_eq!(normalize_server_url(""), DEFAULT_SERVER_URL);
        assert_eq!(normalize_server_url("   "), DEFAULT_SERVER_URL);
    }

    #[test]
    fn normalize_server_url_strips_trailing_slashes() {
        assert_eq!(
            normalize_server_url(" https://api.example.com/// "),
            "https://api.example.com"
        );
    }

    #[test]
    fn normalize_server_url_migrates_legacy_default_http_origin_to_https() {
        assert_eq!(
            normalize_server_url(" http://www.axaihub.com/// "),
            DEFAULT_SERVER_URL
        );
    }
}
