use crate::webdav::WebdavClient;
use chrono::{SecondsFormat, Utc};
use log::debug;
use reqwest::StatusCode;
use std::sync::Arc;
use warp::{
    hyper::body::Bytes,
    reject::{Reject, Rejection},
    Filter,
};

pub async fn run(webdav: WebdavClient) {
    let webdav = Arc::new(webdav);

    let health_probe = warp::get()
        .and(warp::path("health"))
        .then(|| async move { StatusCode::OK });

    let head_root = warp::head()
        .and(warp::path::end())
        .then(|| async move { StatusCode::OK });

    let head = warp::head().then(|| async move { StatusCode::NOT_FOUND });

    let store = warp::put()
        .and(warp::path!("Image.pdf"))
        .and(warp::body::bytes())
        .and_then(move |bytes| store_file(webdav.clone(), bytes));

    let routes = head_root
        .or(head)
        .or(store)
        .or(health_probe)
        .with(warp::log("scan2webdav::http"));

    let signal = async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to await CTRL+C");
    };

    let (_, server) = warp::serve(routes).bind_with_graceful_shutdown(([0, 0, 0, 0], 3030), signal);

    server.await
}

async fn store_file(
    webdav: Arc<WebdavClient>,
    bytes: Bytes,
) -> Result<impl warp::Reply, Rejection> {
    let id = Utc::now()
        .to_rfc3339_opts(SecondsFormat::Secs, true)
        .replace(":", "-");
    let path = format!("EpicPrinter-{id}.pdf");

    debug!("put\t{id} (len = {})", bytes.len());
    webdav.put(path, bytes).await.map_err(internal_error)?;

    Ok(StatusCode::OK)
}

#[derive(Debug)]
struct InternalError {
    #[allow(dead_code)]
    message: String,
}

/// Rejects a request with `404 Not Found`.
#[inline]
fn internal_error(error: impl std::error::Error) -> Rejection {
    warp::reject::custom(InternalError {
        message: error.to_string(),
    })
}

impl Reject for InternalError {}
