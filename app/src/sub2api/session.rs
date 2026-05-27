use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use warpui::{Entity, ModelContext, SingletonEntity};
use warpui_extras::secure_storage::{self, AppContextExt};

use crate::settings::normalize_server_url;

use super::client::Sub2ApiLoginResponse;

const SECURE_STORAGE_KEY: &str = "Sub2ApiSession";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sub2ApiSessionEvent {
    SessionChanged,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sub2ApiSession {
    pub server_url: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub email: String,
}

impl Sub2ApiSession {
    pub fn is_valid(&self) -> bool {
        !self.access_token.is_empty()
            && self
                .expires_at
                .map(|expires_at| expires_at > Utc::now() + Duration::seconds(30))
                .unwrap_or(true)
    }
}

pub struct Sub2ApiSessionStore {
    session: Option<Sub2ApiSession>,
}

impl Sub2ApiSessionStore {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        Self {
            session: Self::load_from_storage(ctx),
        }
    }

    pub fn session(&self) -> Option<&Sub2ApiSession> {
        self.session.as_ref()
    }

    pub fn is_logged_in(&self) -> bool {
        self.session.as_ref().is_some_and(Sub2ApiSession::is_valid)
    }

    pub fn access_token(&self) -> Option<&str> {
        self.session()
            .filter(|session| session.is_valid())
            .map(|session| session.access_token.as_str())
    }

    pub fn server_url(&self) -> Option<&str> {
        self.session().map(|session| session.server_url.as_str())
    }

    pub fn set_login_response(
        &mut self,
        server_url: &str,
        email: &str,
        response: Sub2ApiLoginResponse,
        ctx: &mut ModelContext<Self>,
    ) {
        let expires_at = response
            .expires_in
            .filter(|seconds| *seconds > 0)
            .map(|seconds| Utc::now() + Duration::seconds(seconds));
        let session = Sub2ApiSession {
            server_url: normalize_server_url(server_url),
            access_token: response.token,
            refresh_token: response.refresh_token,
            expires_at,
            email: email.to_owned(),
        };
        self.session = Some(session);
        self.persist(ctx);
        ctx.emit(Sub2ApiSessionEvent::SessionChanged);
    }

    pub fn clear(&mut self, ctx: &mut ModelContext<Self>) {
        self.session = None;
        if let Err(e) = ctx.secure_storage().remove_value(SECURE_STORAGE_KEY) {
            if !matches!(e, secure_storage::Error::NotFound) {
                log::error!("Failed to remove sub2api session: {e:#}");
            }
        }
        ctx.emit(Sub2ApiSessionEvent::SessionChanged);
    }

    fn load_from_storage(ctx: &mut ModelContext<Self>) -> Option<Sub2ApiSession> {
        let raw = match ctx.secure_storage().read_value(SECURE_STORAGE_KEY) {
            Ok(value) => value,
            Err(secure_storage::Error::NotFound) => return None,
            Err(e) => {
                log::error!("Failed to read sub2api session: {e:#}");
                return None;
            }
        };
        match serde_json::from_str(&raw) {
            Ok(session) => Some(session),
            Err(e) => {
                log::error!("Failed to deserialize sub2api session: {e:#}");
                None
            }
        }
    }

    fn persist(&self, ctx: &mut ModelContext<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let json = match serde_json::to_string(session) {
            Ok(json) => json,
            Err(e) => {
                log::error!("Failed to serialize sub2api session: {e:#}");
                return;
            }
        };
        if let Err(e) = ctx.secure_storage().write_value(SECURE_STORAGE_KEY, &json) {
            log::error!("Failed to write sub2api session: {e:#}");
        }
    }
}

impl Entity for Sub2ApiSessionStore {
    type Event = Sub2ApiSessionEvent;
}

impl SingletonEntity for Sub2ApiSessionStore {}
