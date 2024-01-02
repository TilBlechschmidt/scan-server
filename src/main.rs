use webdav::WebdavClient;

mod http;
mod webdav;

#[tokio::main]
async fn main() {
    env_logger::init();

    let endpoint = std::env::var("WEBDAV_URL").expect("No WebDAV URL provided");
    let user = std::env::var("WEBDAV_USER").expect("No WebDAV user provided");
    let pass = std::env::var("WEBDAV_PASS").expect("No WebDAV password provided");

    let client =
        WebdavClient::new(endpoint, user, pass).expect("failed to construct WebDAV client");

    http::run(client).await;
}
