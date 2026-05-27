use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::settings::normalize_server_url;

const LOGIN_ENDPOINT_PATH: &str = "/api/v1/auth/login";
const BOOTSTRAP_ENDPOINT_PATH: &str = "/api/v1/openwarp/bootstrap";
const MAX_LOG_BODY_LEN: usize = 2048;

pub struct Sub2ApiClient {
    client: http_client::Client,
    server_url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Sub2ApiClientError {
    #[error("request failed: {0}")]
    Request(#[from] anyhow::Error),
    #[error("sub2api returned {status}: {message}")]
    Api {
        status: http_client::StatusCode,
        message: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct Sub2ApiProfile {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub balance: f64,
    #[serde(default)]
    pub concurrency: i64,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Sub2ApiGroup {
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Sub2ApiKey {
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub group: Option<Sub2ApiGroup>,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct Sub2ApiBootstrap {
    #[serde(default)]
    pub profile: Sub2ApiProfile,
    #[serde(default)]
    pub api_base_url: String,
    #[serde(default)]
    pub keys: Vec<Sub2ApiKey>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Sub2ApiLoginResponse {
    #[serde(default, alias = "access_token")]
    pub token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_in: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
    data: Option<T>,
}

#[derive(Debug, Serialize)]
struct LoginRequest<'a> {
    email: &'a str,
    password: &'a str,
}

impl Sub2ApiClient {
    pub fn new(server_url: impl AsRef<str>) -> Self {
        Self {
            client: http_client::Client::new(),
            server_url: normalize_server_url(server_url.as_ref()),
        }
    }

    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    pub async fn login(
        &self,
        email: &str,
        password: &str,
    ) -> Result<Sub2ApiLoginResponse, Sub2ApiClientError> {
        let url = login_endpoint_url(&self.server_url);
        log::info!(
            "sub2api login request: method=POST url={} email={} body={}",
            url,
            email_for_log(email),
            login_request_body_for_log(email)
        );
        let response = self
            .client
            .post(&url)
            .json(&LoginRequest { email, password })
            .send()
            .await
            .map_err(|e| {
                log::warn!(
                    "sub2api login request failed before response: method=POST url={} email={} error={}",
                    url,
                    email_for_log(email),
                    e
                );
                Sub2ApiClientError::Request(e.into())
            })?;
        parse_response_envelope("login", &url, response).await
    }

    pub async fn bootstrap(
        &self,
        access_token: &str,
    ) -> Result<Sub2ApiBootstrap, Sub2ApiClientError> {
        let url = self.url(BOOTSTRAP_ENDPOINT_PATH);
        log::info!("sub2api bootstrap request: method=GET url={}", url);
        let response = self
            .client
            .get(&url)
            .header(http_client::AUTHORIZATION, format!("Bearer {access_token}"))
            .send()
            .await
            .map_err(|e| {
                log::warn!(
                    "sub2api bootstrap request failed before response: method=GET url={} error={}",
                    url,
                    e
                );
                Sub2ApiClientError::Request(e.into())
            })?;
        parse_response_envelope("bootstrap", &url, response).await
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.server_url, path)
    }
}

fn login_endpoint_url(server_url: &str) -> String {
    format!(
        "{}{}",
        normalize_server_url(server_url),
        LOGIN_ENDPOINT_PATH
    )
}

async fn parse_response_envelope<T: serde::de::DeserializeOwned>(
    operation: &str,
    url: &str,
    response: http_client::Response,
) -> Result<T, Sub2ApiClientError> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| Sub2ApiClientError::Request(e.into()))?;
    if status.is_success() {
        let envelope = serde_json::from_str::<ApiEnvelope<T>>(&body)
            .map_err(|e| Sub2ApiClientError::Request(e.into()));
        return match envelope? {
            ApiEnvelope {
                code: 0,
                data: Some(data),
                ..
            } => {
                log::info!(
                    "sub2api {} response: url={} status={} code=0 data_present=true",
                    operation,
                    url,
                    status
                );
                Ok(data)
            }
            ApiEnvelope { code, message, .. } => {
                log::warn!(
                    "sub2api {} response error: url={} status={} code={} message={} body={}",
                    operation,
                    url,
                    status,
                    code,
                    message,
                    response_body_for_log(&body)
                );
                Err(Sub2ApiClientError::Api {
                    status,
                    message: if message.is_empty() {
                        format!("unexpected response code {code}")
                    } else {
                        message
                    },
                })
            }
        };
    }

    let message = if body.trim().is_empty() {
        "request failed".to_owned()
    } else {
        body
    };
    log::warn!(
        "sub2api {} response failed: url={} status={} body={}",
        operation,
        url,
        status,
        response_body_for_log(&message)
    );
    Err(Sub2ApiClientError::Api { status, message })
}

fn email_for_log(email: &str) -> String {
    let trimmed = email.trim();
    let Some((local, domain)) = trimmed.split_once('@') else {
        return "<invalid-email>".to_owned();
    };
    let mut chars = local.chars();
    let first = chars.next().unwrap_or('*');
    format!("{first}***@{domain}")
}

fn login_request_body_for_log(email: &str) -> String {
    serde_json::json!({
        "email": email_for_log(email),
        "password": "<redacted>"
    })
    .to_string()
}

fn response_body_for_log(body: &str) -> String {
    match serde_json::from_str::<Value>(body) {
        Ok(mut value) => {
            redact_sensitive_json(&mut value);
            truncate_for_log(&value.to_string())
        }
        Err(_) => truncate_for_log(body),
    }
}

fn redact_sensitive_json(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                if is_sensitive_key(key) {
                    *value = Value::String("<redacted>".to_owned());
                } else {
                    redact_sensitive_json(value);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_sensitive_json(item);
            }
        }
        _ => {}
    }
}

fn is_sensitive_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "password" | "token" | "access_token" | "refresh_token"
    )
}

fn truncate_for_log(value: &str) -> String {
    let mut out: String = value.chars().take(MAX_LOG_BODY_LEN).collect();
    if out.len() < value.len() {
        out.push_str("...<truncated>");
    }
    out
}

fn deserialize_string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(value) => Ok(value),
        Value::Number(value) => Ok(value.to_string()),
        Value::Null => Ok(String::new()),
        other => Err(serde::de::Error::custom(format!(
            "expected string or number, got {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::{login_endpoint_url, response_body_for_log, ApiEnvelope, Sub2ApiBootstrap};

    #[test]
    fn login_endpoint_url_matches_sub2api_auth_route() {
        assert_eq!(
            login_endpoint_url(" https://www.axaihub.com/// "),
            "https://www.axaihub.com/api/v1/auth/login"
        );
    }

    #[test]
    fn response_body_for_log_redacts_sensitive_json_fields() {
        let logged = response_body_for_log(
            r#"{"code":0,"data":{"access_token":"secret","refresh_token":"refresh","nested":{"password":"pw"}}}"#,
        );

        assert!(logged.contains("\"access_token\":\"<redacted>\""));
        assert!(logged.contains("\"refresh_token\":\"<redacted>\""));
        assert!(logged.contains("\"password\":\"<redacted>\""));
        assert!(!logged.contains(":\"secret\""));
        assert!(!logged.contains(":\"refresh\""));
        assert!(!logged.contains(":\"pw\""));
    }

    #[test]
    fn bootstrap_accepts_numeric_ids_from_backend_contract() {
        let envelope: ApiEnvelope<Sub2ApiBootstrap> = serde_json::from_str(
            r#"{
                "code": 0,
                "message": "success",
                "data": {
                    "profile": {
                        "email": "user@example.com",
                        "username": "user",
                        "balance": 1.5,
                        "concurrency": 2,
                        "status": "active"
                    },
                    "api_base_url": "https://api.example.com",
                    "keys": [{
                        "id": 33,
                        "name": "key",
                        "key": "sk-test",
                        "status": "active",
                        "group": {
                            "id": 7,
                            "name": "OpenAI",
                            "platform": "openai"
                        },
                        "models": ["gpt-5"]
                    }]
                }
            }"#,
        )
        .expect("numeric ids should deserialize");

        let bootstrap = envelope.data.unwrap();
        assert_eq!(bootstrap.keys[0].id, "33");
        assert_eq!(bootstrap.keys[0].group.as_ref().unwrap().id, "7");
    }
}
