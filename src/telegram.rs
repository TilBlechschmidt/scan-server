use reqwest::Client;
use std::{borrow::Cow, collections::HashMap};

type CowStr = Cow<'static, str>;

#[derive(Clone)]
pub struct TelegramClient {
    http_client: Client,

    chat: CowStr,
    token: CowStr,
}

impl TelegramClient {
    pub fn new(chat: impl Into<CowStr>, token: impl Into<CowStr>) -> reqwest::Result<Self> {
        Ok(Self {
            http_client: reqwest::ClientBuilder::new()
                .danger_accept_invalid_certs(true)
                .build()
                .unwrap(), // Client::new(),
            chat: chat.into(),
            token: token.into(),
        })
    }

    pub async fn send(&self, message: impl AsRef<str>) -> reqwest::Result<()> {
        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.token);

        let mut params = HashMap::new();
        params.insert("parse_mode", "HTML".to_string());
        params.insert("chat_id", self.chat.to_string());
        params.insert("text", message.as_ref().to_string());

        self.http_client
            .post(url)
            .form(&params)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}
