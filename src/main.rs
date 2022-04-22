use serde::Serialize;
use std::{path::PathBuf, time::SystemTime};
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

    let routes = fetch.or(store).or(delete).or(index);

    warp::serve(routes).run(([0, 0, 0, 0], 3030)).await;
}

async fn build_index(storage_path: &str) -> Result<Json, Rejection> {
    let mut entries: Vec<FileEntry> = vec![];
    let mut dir = tokio::fs::read_dir(storage_path)
        .await
        .map_err(internal_error)?;

    while let Some(entry) = dir.next_entry().await.map_err(internal_error)? {
        let file_type = entry.file_type().await.map_err(internal_error)?;
        let file_name = entry.file_name().to_string_lossy().into_owned();

        let metadata = entry.metadata().await.map_err(internal_error)?;
        let creation_time = metadata.created().map_err(internal_error)?;
        let timestamp = creation_time
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(internal_error)?
            .as_secs();

        if file_type.is_file() {
            entries.push(FileEntry {
                path: format!("/document/{file_name}"),
                scanned_at: format!("{timestamp}"),
            });
        }
    }

    Ok(warp::reply::json(&entries))
}

async fn delete_file(storage_path: &str, file_name: String) -> Result<impl warp::Reply, Rejection> {
    let storage_path: PathBuf = storage_path.into();
    let path = storage_path.join(file_name.to_string());

    println!("del\t{file_name}");

    tokio::fs::remove_file(path).await.map_err(internal_error)?;

    Ok(StatusCode::GONE)
}

async fn store_file(storage_path: &str, bytes: Bytes) -> Result<impl warp::Reply, Rejection> {
    let id = uuid::Uuid::new_v4();

    println!("put\t{id} (len = {})", bytes.len());

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
