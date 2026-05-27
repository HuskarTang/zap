use std::collections::HashMap;

use settings::Setting;
use warpui::{ModelContext, SingletonEntity, ViewContext};

use crate::ai::agent_providers::openai_compatible::OpenAiCompatibleModel;
use crate::ai::agent_providers::AgentProviderSecrets;
use crate::settings::{AISettings, AgentProvider, AgentProviderApiType, AgentProviderModel};

use super::client::{Sub2ApiBootstrap, Sub2ApiKey};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sub2ApiImportReport {
    pub imported: usize,
    pub skipped: usize,
    pub provider_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sub2ApiKeyImportState {
    Imported,
    Importable,
    Unavailable,
}

pub fn provider_for_key(api_base_url: &str, key: &Sub2ApiKey) -> Option<AgentProvider> {
    if !key.status.eq_ignore_ascii_case("active") || key.key.trim().is_empty() {
        return None;
    }

    let group = key.group.as_ref()?;
    let api_type = api_type_for_platform(group.platform.as_deref());
    let base_url = base_url_for_platform(api_base_url, group.platform.as_deref());
    let name = if key.name.trim().is_empty() {
        group
            .name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("sub2api")
            .to_owned()
    } else {
        key.name.clone()
    };

    let models = key
        .models
        .iter()
        .filter(|model| !model.trim().is_empty())
        .map(|model| AgentProviderModel::from_id(model.clone()))
        .collect();

    let mut provider = AgentProvider::new_empty();
    provider.name = name;
    provider.api_type = api_type;
    provider.base_url = base_url;
    provider.models = models;
    Some(provider)
}

pub fn is_key_imported(
    api_base_url: &str,
    key: &Sub2ApiKey,
    existing_providers: &[AgentProvider],
) -> bool {
    key_import_state(api_base_url, key, existing_providers) == Sub2ApiKeyImportState::Imported
}

pub fn key_import_state(
    api_base_url: &str,
    key: &Sub2ApiKey,
    existing_providers: &[AgentProvider],
) -> Sub2ApiKeyImportState {
    let Some(provider) = provider_for_key(api_base_url, key) else {
        return Sub2ApiKeyImportState::Unavailable;
    };

    if existing_providers
        .iter()
        .any(|existing| is_matching_provider(existing, &provider))
    {
        Sub2ApiKeyImportState::Imported
    } else {
        Sub2ApiKeyImportState::Importable
    }
}

pub fn merge_provider_models(
    provider: &mut AgentProvider,
    fetched: Vec<OpenAiCompatibleModel>,
) -> usize {
    let existing: std::collections::HashSet<String> =
        provider.models.iter().map(|m| m.id.clone()).collect();
    let mut added = 0;
    for model in fetched {
        if !existing.contains(&model.id) {
            provider.models.push(AgentProviderModel::from_id(model.id));
            added += 1;
        }
    }
    added
}

pub fn import_bootstrap_keys(
    bootstrap: &Sub2ApiBootstrap,
    existing_providers: Vec<AgentProvider>,
) -> (
    Vec<AgentProvider>,
    HashMap<String, String>,
    Sub2ApiImportReport,
) {
    let mut providers = existing_providers;
    let mut secrets = HashMap::new();
    let mut provider_ids = Vec::new();
    let mut skipped = 0;

    for key in &bootstrap.keys {
        let Some(provider) = provider_for_key(&bootstrap.api_base_url, key) else {
            skipped += 1;
            continue;
        };

        if let Some(existing) = providers
            .iter_mut()
            .find(|existing| is_matching_provider(existing, &provider))
        {
            existing.kind = provider.kind;
            existing.base_url = provider.base_url;
            existing.models = provider.models;
            existing.extra_headers = provider.extra_headers;
            secrets.insert(existing.id.clone(), key.key.clone());
            provider_ids.push(existing.id.clone());
        } else {
            let provider_id = provider.id.clone();
            secrets.insert(provider_id.clone(), key.key.clone());
            provider_ids.push(provider_id);
            providers.push(provider);
        }
    }

    let report = Sub2ApiImportReport {
        imported: provider_ids.len(),
        skipped,
        provider_ids,
    };

    (providers, secrets, report)
}

pub trait ImportIntoSettingsContext {
    fn import_sub2api_bootstrap(&mut self, bootstrap: &Sub2ApiBootstrap) -> Sub2ApiImportReport;
}

impl<T> ImportIntoSettingsContext for ModelContext<'_, T>
where
    T: warpui::Entity,
{
    fn import_sub2api_bootstrap(&mut self, bootstrap: &Sub2ApiBootstrap) -> Sub2ApiImportReport {
        let existing = AISettings::as_ref(self).agent_providers.value().clone();
        let (providers, secrets, report) = import_bootstrap_keys(bootstrap, existing);

        AISettings::handle(self).update(self, |settings, ctx| {
            let _ = settings.agent_providers.set_value(providers, ctx);
        });

        AgentProviderSecrets::handle(self).update(self, |store, ctx| {
            for (provider_id, api_key) in secrets {
                store.set(&provider_id, api_key, ctx);
            }
        });

        report
    }
}

impl<T> ImportIntoSettingsContext for ViewContext<'_, T>
where
    T: warpui::View + warpui::Entity,
{
    fn import_sub2api_bootstrap(&mut self, bootstrap: &Sub2ApiBootstrap) -> Sub2ApiImportReport {
        let existing = AISettings::as_ref(self).agent_providers.value().clone();
        let (providers, secrets, report) = import_bootstrap_keys(bootstrap, existing);

        AISettings::handle(self).update(self, |settings, ctx| {
            let _ = settings.agent_providers.set_value(providers, ctx);
        });

        AgentProviderSecrets::handle(self).update(self, |store, ctx| {
            for (provider_id, api_key) in secrets {
                store.set(&provider_id, api_key, ctx);
            }
        });

        report
    }
}

fn api_type_for_platform(platform: Option<&str>) -> AgentProviderApiType {
    match platform.unwrap_or_default().to_ascii_lowercase().as_str() {
        "anthropic" | "claude" => AgentProviderApiType::Anthropic,
        "gemini" | "google" => AgentProviderApiType::Gemini,
        "ollama" => AgentProviderApiType::Ollama,
        "deepseek" => AgentProviderApiType::DeepSeek,
        _ => AgentProviderApiType::OpenAi,
    }
}

fn base_url_for_platform(api_base_url: &str, platform: Option<&str>) -> String {
    let base = api_base_url.trim().trim_end_matches('/');
    let base = if base.is_empty() {
        crate::settings::DEFAULT_SERVER_URL.trim_end_matches('/')
    } else {
        base
    };

    let platform = platform.unwrap_or_default().to_ascii_lowercase();
    let normalized = match platform.as_str() {
        "anthropic" | "claude" => append_path_suffix(base, "/anthropic"),
        "gemini" | "google" => append_path_suffix(base, "/gemini"),
        "antigravity" => append_path_suffix(base, "/antigravity"),
        path if path.starts_with('/') => append_path_suffix(base, path),
        _ => append_path_suffix(base, "/v1"),
    };

    format!("{normalized}/")
}

fn append_path_suffix(base: &str, suffix: &str) -> String {
    let base = base.trim_end_matches('/');
    let suffix = suffix.trim();
    if suffix.is_empty() {
        return base.to_owned();
    }

    let suffix_without_slash = suffix.trim_start_matches('/');
    if base
        .rsplit('/')
        .next()
        .is_some_and(|segment| segment.eq_ignore_ascii_case(suffix_without_slash))
    {
        base.to_owned()
    } else {
        format!("{base}/{suffix_without_slash}")
    }
}

fn normalize_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_owned()
}

fn is_matching_provider(existing: &AgentProvider, provider: &AgentProvider) -> bool {
    existing.name == provider.name
        && existing.api_type == provider.api_type
        && normalize_base_url(&existing.base_url) == normalize_base_url(&provider.base_url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sub2api::client::Sub2ApiGroup;

    fn key(id: &str, name: &str, platform: &str, secret: &str, models: &[&str]) -> Sub2ApiKey {
        Sub2ApiKey {
            id: id.to_owned(),
            name: name.to_owned(),
            key: secret.to_owned(),
            status: "active".to_owned(),
            group: Some(Sub2ApiGroup {
                id: "g1".to_owned(),
                name: Some(format!("{platform} group")),
                platform: Some(platform.to_owned()),
            }),
            models: models.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn provider_for_key_maps_platform_and_gateway_url() {
        let provider = provider_for_key(
            "https://api2.veilnet.icu",
            &key("500", "sub500", "openai", "sk-1", &["gpt-5"]),
        )
        .unwrap();

        assert!(uuid::Uuid::parse_str(&provider.id).is_ok());
        assert_eq!(provider.name, "sub500");
        assert_eq!(provider.api_type, AgentProviderApiType::OpenAi);
        assert_eq!(provider.base_url, "https://api2.veilnet.icu/v1/");
        assert_eq!(provider.models[0].id, "gpt-5");
    }

    #[test]
    fn provider_for_key_maps_non_openai_platforms() {
        let provider = provider_for_key(
            "https://api2.veilnet.icu/",
            &key("claude", "claude", "anthropic", "sk-1", &["claude-sonnet"]),
        )
        .unwrap();

        assert_eq!(provider.api_type, AgentProviderApiType::Anthropic);
        assert_eq!(provider.base_url, "https://api2.veilnet.icu/anthropic/");
    }

    #[test]
    fn provider_for_key_does_not_duplicate_v1_suffix() {
        let provider = provider_for_key(
            "https://api2.veilnet.icu/v1",
            &key("500", "sub500", "openai", "sk-1", &["gpt-5"]),
        )
        .unwrap();

        assert_eq!(provider.base_url, "https://api2.veilnet.icu/v1/");
    }

    #[test]
    fn import_bootstrap_keys_upserts_multiple_keys_and_keeps_manual_providers() {
        let manual = AgentProvider {
            id: "manual".to_owned(),
            name: "Manual".to_owned(),
            kind: Default::default(),
            api_type: AgentProviderApiType::OpenAi,
            base_url: "https://manual/v1/".to_owned(),
            models: vec![AgentProviderModel::from_id("manual-model".to_owned())],
            extra_headers: Vec::new(),
        };
        let bootstrap = Sub2ApiBootstrap {
            profile: Default::default(),
            api_base_url: "https://api2.veilnet.icu".to_owned(),
            keys: vec![
                key("500", "sub500", "openai", "sk-500", &["gpt-5"]),
                key("0", "tlykey0", "openai", "sk-0", &["gpt-4o"]),
            ],
        };

        let (providers, secrets, report) = import_bootstrap_keys(&bootstrap, vec![manual.clone()]);

        assert_eq!(report.imported, 2);
        assert_eq!(providers.len(), 3);
        assert!(providers.iter().any(|p| p.id == "manual"));
        assert_eq!(secrets.len(), 2);
        assert!(secrets.values().any(|secret| secret == "sk-500"));
        assert!(secrets.values().any(|secret| secret == "sk-0"));
    }

    #[test]
    fn import_bootstrap_keys_updates_matching_provider_without_new_id() {
        let mut existing = AgentProvider::new_empty();
        existing.name = "sub500".to_owned();
        existing.api_type = AgentProviderApiType::OpenAi;
        existing.base_url = "https://api2.veilnet.icu/v1".to_owned();
        existing.models = vec![AgentProviderModel::from_id("old-model".to_owned())];
        let existing_id = existing.id.clone();

        let bootstrap = Sub2ApiBootstrap {
            profile: Default::default(),
            api_base_url: "https://api2.veilnet.icu".to_owned(),
            keys: vec![key("500", "sub500", "openai", "sk-500", &["gpt-5"])],
        };

        let (providers, secrets, report) = import_bootstrap_keys(&bootstrap, vec![existing]);

        assert_eq!(report.imported, 1);
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, existing_id);
        assert_eq!(providers[0].models[0].id, "gpt-5");
        assert_eq!(secrets.get(&existing_id).unwrap(), "sk-500");
    }

    #[test]
    fn import_bootstrap_keys_preserves_bootstrap_model_ids() {
        let bootstrap = Sub2ApiBootstrap {
            profile: Default::default(),
            api_base_url: "https://api2.veilnet.icu".to_owned(),
            keys: vec![key(
                "500",
                "sub500",
                "openai",
                "sk-500",
                &["gpt-5.5", "gpt-image-1"],
            )],
        };

        let (providers, _, _) = import_bootstrap_keys(&bootstrap, vec![]);

        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].models.len(), 2);
        assert_eq!(providers[0].models[0].id, "gpt-5.5");
        assert_eq!(providers[0].models[1].id, "gpt-image-1");
    }

    #[test]
    fn is_key_imported_matches_existing_provider_contract() {
        let mut existing = AgentProvider::new_empty();
        existing.name = "sub500".to_owned();
        existing.api_type = AgentProviderApiType::OpenAi;
        existing.base_url = "https://api2.veilnet.icu/v1/".to_owned();

        assert!(is_key_imported(
            "https://api2.veilnet.icu",
            &key("500", "sub500", "openai", "sk-500", &["gpt-5"]),
            &[existing]
        ));
    }

    #[test]
    fn key_import_state_distinguishes_imported_importable_and_unavailable() {
        let mut existing = AgentProvider::new_empty();
        existing.name = "sub500".to_owned();
        existing.api_type = AgentProviderApiType::OpenAi;
        existing.base_url = "https://api2.veilnet.icu/v1/".to_owned();

        assert_eq!(
            key_import_state(
                "https://api2.veilnet.icu",
                &key("500", "sub500", "openai", "sk-500", &["gpt-5"]),
                &[existing]
            ),
            Sub2ApiKeyImportState::Imported
        );

        assert_eq!(
            key_import_state(
                "https://api2.veilnet.icu",
                &key("501", "sub501", "openai", "sk-501", &["gpt-4o"]),
                &[]
            ),
            Sub2ApiKeyImportState::Importable
        );

        let mut inactive = key("502", "sub502", "openai", "sk-502", &["gpt-4.1"]);
        inactive.status = "disabled".to_owned();
        assert_eq!(
            key_import_state("https://api2.veilnet.icu", &inactive, &[]),
            Sub2ApiKeyImportState::Unavailable
        );
    }

    #[test]
    fn merge_provider_models_keeps_existing_and_appends_new_ids() {
        let mut provider = AgentProvider::new_empty();
        provider.models = vec![AgentProviderModel::from_id("gpt-5.5".to_owned())];

        let added = merge_provider_models(
            &mut provider,
            vec![
                OpenAiCompatibleModel {
                    id: "gpt-5.5".to_owned(),
                    owned_by: None,
                },
                OpenAiCompatibleModel {
                    id: "gpt-image-1".to_owned(),
                    owned_by: None,
                },
            ],
        );

        assert_eq!(added, 1);
        assert_eq!(provider.models.len(), 2);
        assert_eq!(provider.models[0].id, "gpt-5.5");
        assert_eq!(provider.models[1].id, "gpt-image-1");
    }
}
