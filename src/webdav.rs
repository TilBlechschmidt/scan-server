use reqwest::{Body, Client, IntoUrl, Method, Url};
use std::borrow::Cow;

type CowStr = Cow<'static, str>;

pub struct WebdavClient {
    http_client: Client,
    endpoint: Url,
    user: CowStr,
    pass: CowStr,
}

impl WebdavClient {
    pub fn new<U, S>(endpoint: U, user: S, pass: S) -> reqwest::Result<Self>
    where
        U: IntoUrl,
        S: Into<CowStr>,
    {
        Ok(Self {
            http_client: Client::new(),
            endpoint: endpoint.into_url()?,
            user: user.into(),
            pass: pass.into(),
        })
    }

    pub async fn put<S, B>(&self, path: S, body: B) -> reqwest::Result<()>
    where
        S: Into<CowStr>,
        B: Into<Body>,
    {
        let mut url = self.endpoint.clone();
        url.path_segments_mut().unwrap().push(&path.into());

        self.http_client
            .request(Method::PUT, url)
            .basic_auth(&self.user, Some(&self.pass))
            .body(body)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}
