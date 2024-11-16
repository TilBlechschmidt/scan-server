use reqwest::Body;
use user::UserMap;

mod http;
mod paperless;
mod telegram;
mod user;
mod webdav;

#[tokio::main]
async fn main() {
    env_logger::init();

    let users = UserMap::from_env();

    http::run(users).await;
}

#[trait_variant::make(StorageBackend: Send)]
trait LocalStorageBackend {
    async fn put(
        &self,
        id: &str,
        body: Body,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}
