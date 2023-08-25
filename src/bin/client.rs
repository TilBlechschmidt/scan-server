use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::path::PathBuf;
use tokio::{fs::File, io::AsyncWriteExt};

#[derive(Debug, Deserialize)]
struct FileEntry {
    path: String,
    scanned_at: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = std::env::var("SCAN_TOKEN").expect("missing SCAN_TOKEN env var");
    let auth = format!("Bearer {token}");
    let path: PathBuf = "/Users/tibl/Downloads".into();

    let client = Client::new();
    let body = client
        .get("https://scan.tibl.dev")
        .header("Authorization", &auth)
        .send()
        .await?
        .text()
        .await?;

    println!("BODY:\n{body}");

    let files: Vec<FileEntry> = serde_json::from_str(&body).unwrap();

    for file_entry in files {
        println!("Downloading file {}", file_entry.path);

        // Download file
        let file_url = format!("https://scan.tibl.dev{}", file_entry.path);
        let file_bytes = client
            .get(&file_url)
            .header("Authorization", &auth)
            .send()
            .await?
            .bytes()
            .await?;

        // Write to disk
        let file_name = format!("{}.pdf", file_entry.scanned_at);
        let file_path = path.join(file_name);
        let mut file = File::create(file_path).await?;
        file.write_all(&file_bytes).await?;
        file.flush().await?;

        // Delete online version
        let delete_status = client
            .delete(&file_url)
            .header("Authorization", &auth)
            .send()
            .await?
            .status();

        if delete_status != StatusCode::GONE {
            eprintln!("Failed to delete file {}", file_entry.path);
        }
    }

    Ok(())
}
