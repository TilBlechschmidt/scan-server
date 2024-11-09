use user::UserMap;

mod http;
mod paperless;
mod user;
mod webdav;

#[tokio::main]
async fn main() {
    env_logger::init();

    let users = UserMap::from_env();

    http::run(users).await;
}
