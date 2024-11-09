use crate::{http::internal_error, paperless::PaperlessClient, webdav::WebdavClient};
use chrono::{SecondsFormat, Utc};
use log::debug;
use reqwest::StatusCode;
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

pub struct User {
    name: String,
    webdav: Option<WebdavClient>,
    paperless: Option<PaperlessClient>,
}

impl User {
    pub fn new(
        name: String,
        webdav: Option<WebdavClient>,
        paperless: Option<PaperlessClient>,
    ) -> Self {
        Self {
            name: name.to_lowercase(),
            webdav,
            paperless,
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

        Self::new(name, webdav, paperless)
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

        if let Some(webdav) = &self.webdav {
            debug!("{id}\tCalling WebDAV ...");
            webdav
                .put(format!("EpicPrinter-{id}.pdf"), bytes.clone())
                .await
                .map_err(internal_error)?;
        }

        if let Some(paperless) = &self.paperless {
            debug!("{id}\tCalling Paperless ...");
            paperless.put(id, bytes).await.map_err(internal_error)?;
        }

        Ok(StatusCode::OK)
    }
}
