use chrono::Utc;
use log::debug;
use serde::Serialize;
use std::path::PathBuf;
use tokio::{
    fs::{create_dir_all, File},
    io::AsyncWriteExt,
};
use warp::{
    hyper::{body::Bytes, StatusCode},
    reject::Reject,
    reply::Json,
    Filter, Rejection,
};

#[tokio::main]
async fn main() {
    env_logger::init();

    // Parse arguments from environment by leaking their memory and converting them to `&'static str` instances
    let storage_path =
        string_to_static_str(std::env::var("STORAGE_PATH").expect("No storage path provided."));
    let auth_token =
        string_to_static_str(std::env::var("AUTH_TOKEN").expect("No auth token provided."));
    let content_length_limit = 1024 * 1024 * 256;

    create_dir_all(storage_path)
        .await
        .expect("failed to create storage directory");

    let auth = warp::header::exact("Authorization", auth_token);

    let fetch = warp::path("document")
        .and(auth)
        .and(warp::fs::dir(storage_path));

    let store = warp::put()
        .and(warp::path!("Image.pdf"))
        .and(warp::body::content_length_limit(content_length_limit))
        .and(warp::body::bytes())
        .and_then(move |bytes| store_file(&storage_path, bytes));

    let delete = warp::delete()
        .and(warp::path!("document" / String))
        .and(auth)
        .and_then(move |file_name| delete_file(&storage_path, file_name));

    let index = warp::get()
        .and(warp::path::end())
        .and(auth)
        .and_then(move || build_index(&storage_path));

    let health_probe = warp::get()
        .and(warp::path("health"))
        .then(|| async move { StatusCode::OK });

    let head_root = warp::head()
        .and(warp::path::end())
        .then(|| async move { StatusCode::OK });

    let head = warp::head().then(|| async move { StatusCode::NOT_FOUND });

    let routes = head_root
        .or(head)
        .or(fetch)
        .or(store)
        .or(delete)
        .or(index)
        .or(health_probe)
        .with(warp::log("scan-server::http"));

    let signal = async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to await CTRL+C");
    };

    let (_, server) = warp::serve(routes).bind_with_graceful_shutdown(([0, 0, 0, 0], 3030), signal);

    server.await
}

async fn build_index(storage_path: &str) -> Result<Json, Rejection> {
    let mut entries: Vec<FileEntry> = vec![];
    let mut dir = tokio::fs::read_dir(storage_path)
        .await
        .map_err(internal_error)?;

    while let Some(entry) = dir.next_entry().await.map_err(internal_error)? {
        let file_type = entry.file_type().await.map_err(internal_error)?;
        let file_name = entry.file_name().to_string_lossy().into_owned();

        if file_type.is_file() {
            entries.push(FileEntry {
                path: format!("/document/{file_name}"),
                // Since we use the RFC3339 formatted upload date as the filename, it can be used here directly
                scanned_at: format!("{file_name}"),
            });
        }
    }

    Ok(warp::reply::json(&entries))
}

async fn delete_file(storage_path: &str, file_name: String) -> Result<impl warp::Reply, Rejection> {
    let storage_path: PathBuf = storage_path.into();
    let path = storage_path.join(file_name.to_string());

    debug!("del\t{file_name}");

    tokio::fs::remove_file(path).await.map_err(internal_error)?;

    Ok(StatusCode::GONE)
}

async fn store_file(storage_path: &str, bytes: Bytes) -> Result<impl warp::Reply, Rejection> {
    let id = Utc::now().to_rfc3339();

    debug!("put\t{id} (len = {})", bytes.len());

    let storage_path: PathBuf = storage_path.into();
    let path = storage_path.join(id.to_string());

    let mut file = File::create(path).await.map_err(internal_error)?;
    file.write_all(&bytes).await.map_err(internal_error)?;
    file.flush().await.map_err(internal_error)?;

    Ok(StatusCode::OK)
}

#[derive(Serialize)]
struct FileEntry {
    path: String,
    scanned_at: String,
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

// Dirty fix to retain CLI arguments for the lifetime of the program.
// Arguably that is the only situation where leaking memory is okay.
// Still not great, but simple.
fn string_to_static_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}
