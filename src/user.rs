use crate::{
    http::internal_error, paperless::PaperlessClient, telegram::TelegramClient,
    webdav::WebdavClient, StorageBackend,
};
use chrono::{SecondsFormat, Utc};
use log::{debug, error};
use reqwest::{Body, StatusCode};
use std::{collections::HashMap, env, ops::Deref, sync::Arc};
use warp::{hyper::body::Bytes, reject::Rejection};

#[derive(Clone)]
pub struct UserMap(Arc<HashMap<String, User>>);

impl UserMap {
    pub fn from_env() -> Self {
        let users = env::var("SCAN_USERS")
            .expect("No users provided")
            .split(",")
            .map(str::trim)
            .map(str::to_lowercase)
            .map(|name| (name.clone(), User::from_env(name)))
            .collect();

        Self(Arc::new(users))
    }
}

impl Deref for UserMap {
    type Target = Arc<HashMap<String, User>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone)]
pub struct User {
    name: String,
    webdav: Option<WebdavClient>,
    paperless: Option<PaperlessClient>,
    telegram: Option<TelegramClient>,
}

impl User {
    pub fn new(
        name: String,
        webdav: Option<WebdavClient>,
        paperless: Option<PaperlessClient>,
        telegram: Option<TelegramClient>,
    ) -> Self {
        Self {
            name: name.to_lowercase(),
            webdav,
            paperless,
            telegram,
        }
    }

    pub fn from_env(name: String) -> Self {
        let u = name.to_uppercase();

        let webdav = env::var(format!("{u}_WEBDAV_URL")).ok().map(|endpoint| {
            let user = env::var(format!("{u}_WEBDAV_USER")).expect("No WebDAV user provided");
            let pass = env::var(format!("{u}_WEBDAV_PASS")).expect("No WebDAV password provided");

            WebdavClient::new(endpoint, user, pass).expect("Failed to construct WebDAV client")
        });

        let paperless = env::var(format!("{u}_PAPERLESS_URL")).ok().map(|endpoint| {
            let token =
                env::var(format!("{u}_PAPERLESS_TOKEN")).expect("No Paperless token provided");

            let custom_fields_raw =
                &env::var(format!("{u}_PAPERLESS_CUSTOM_FIELDS")).unwrap_or("[]".into());

            let custom_fields = serde_json::from_str(custom_fields_raw)
                .expect("Invalid value for Paperless custom fields");

            PaperlessClient::new(endpoint, token, custom_fields)
                .expect("Failed to construct Paperless client")
        });

        let telegram = env::var(format!("{u}_TELEGRAM_TOKEN")).ok().map(|token| {
            let chat = env::var(format!("{u}_TELEGRAM_CHAT")).expect("Missing telegram chat ID");

            TelegramClient::new(chat, token).expect("Failed to construct Telegram client")
        });

        Self::new(name, webdav, paperless, telegram)
    }

    pub async fn store(&self, bytes: Bytes) -> Result<StatusCode, Rejection> {
        let id = Utc::now()
            .to_rfc3339_opts(SecondsFormat::Secs, true)
            .replace(":", "-");

        debug!(
            "{id}\tStarting upload (user = {}, len = {})",
            self.name,
            bytes.len()
        );

        if let Some(webdav) = self.webdav.clone() {
            debug!("{id}\tCalling WebDAV ...");
            self.store_in_background(id.clone(), bytes.clone().into(), webdav);
        }

        if let Some(paperless) = self.paperless.clone() {
            debug!("{id}\tCalling Paperless ...");
            self.store_in_background(id, bytes.into(), paperless);
        }

        Ok(StatusCode::OK)
    }

    fn store_in_background(
        &self,
        id: String,
        bytes: Body,
        backend: impl StorageBackend + Send + Sync + 'static,
    ) {
        let telegram = self.telegram.clone();

        tokio::spawn(async move {
            let result = backend.put(&id, bytes).await;

            match result {
                Ok(_) => debug!("{id}\tUpload finished"),
                Err(err) => {
                    error!("{id}\tUpload failed: {err:?}");

                    if let Some(telegram) = telegram {
                        if let Err(notify_error) = telegram
                            .send(format!(
                                "<b>EpicPrinter processing failed</b>\nFile: <i>{id}</i>\n\n<blockquote><code>{err}</code></blockquote>"
                            ))
                            .await
                        {
                            error!("{id}\tFailed to notify user of error: {notify_error:?}");
                        }
                    }
                }
            }
        });
    }
}
