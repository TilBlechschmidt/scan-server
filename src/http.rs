use crate::user::UserMap;
use reqwest::StatusCode;
use warp::{
    reject::{Reject, Rejection},
    Filter,
};

pub async fn run(users: UserMap) {
    let health_probe = warp::get()
        .and(warp::path("health"))
        .then(|| async move { StatusCode::OK });

    let head_root = warp::head()
        .and(warp::path::end())
        .then(|| async move { StatusCode::OK });

    let head = warp::head().then(|| async move { StatusCode::NOT_FOUND });

    let store = warp::put()
        .and(warp::path!(String / "Image.pdf"))
        .and(warp::body::bytes())
        .and_then(move |user, bytes| {
            let users = users.clone();

            async move {
                match users.get(&user) {
                    Some(user) => user.store(bytes).await,
                    None => Ok(StatusCode::NOT_FOUND),
                }
            }
        });

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
    log::info!("Listening on 0.0.0.0:3030");

    server.await
}

#[derive(Debug)]
pub struct InternalError {
    #[allow(dead_code)]
    pub message: String,
}

/// Rejects a request with `404 Not Found`.
#[inline]
pub fn internal_error(error: impl ToString) -> Rejection {
    warp::reject::custom(InternalError {
        message: error.to_string(),
    })
}

impl Reject for InternalError {}
