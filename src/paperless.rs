use log::{debug, error};
use reqwest::{
    multipart::{self, Part},
    Body, Client, IntoUrl, Url,
};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    time::{Duration, Instant},
};
use tokio::time::sleep;

type CowStr = Cow<'static, str>;

#[derive(Clone)]
pub struct PaperlessClient {
    http_client: Client,
    endpoint: Url,

    token: CowStr,

    custom_fields: CustomFieldsPatch,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CustomField {
    field: usize,
    value: usize,
}

#[derive(Serialize, Clone)]
struct CustomFieldsPatch {
    custom_fields: Vec<CustomField>,
}

impl PaperlessClient {
    pub fn new<U, S>(
        endpoint: U,
        token: S,
        custom_fields: Vec<CustomField>,
    ) -> reqwest::Result<Self>
    where
        U: IntoUrl,
        S: Into<CowStr>,
    {
        Ok(Self {
            http_client: Client::new(),
            endpoint: endpoint.into_url()?,
            token: token.into(),
            custom_fields: CustomFieldsPatch { custom_fields },
        })
    }

    pub async fn put<S, B>(
        &self,
        title: S,
        body: B,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        S: Into<CowStr>,
        B: Into<Body>,
    {
        let url = self.url(&["api", "documents", "post_document", ""]);

        let title = title.into();

        let file = Part::stream(body)
            .file_name(format!("EpicPrinter-{title}.pdf"))
            .mime_str("application/pdf")?;

        let form = multipart::Form::new()
            .text("title", title.clone())
            .part("document", file);

        let upload_id = self
            .http_client
            .post(url)
            .header("Authorization", format!("Token {}", self.token))
            .multipart(form)
            .send()
            .await?
            .error_for_status()?
            .json::<String>()
            .await?;

        debug!("{title}\tUpload complete (task = {upload_id})");

        let client = self.clone();

        // While running this sync would be nice for error reporting,
        // it would stall the scanning process considerably. This would
        // make bulk scanning really annoying. Additionally, the printer
        // would not properly report the error anyway so we might as well.
        //
        // This runs the risk of not finishing the processing without the
        // user noticing but the impact is rather low so whatever :D
        tokio::spawn(async move {
            match client.processing_task(&upload_id).await {
                Ok(id) => debug!("{title}\tProcessing complete (doc = {id})"),
                Err(err) => error!("{title}\t{err}"),
            }
        });

        Ok(())
    }

    async fn processing_task(
        &self,
        upload_id: &str,
    ) -> Result<DocumentID, Box<dyn std::error::Error + Send + Sync>> {
        let document_id = self.wait_for_processing(&upload_id).await?;

        self.set_custom_fields(&document_id).await?;

        Ok(document_id)
    }

    async fn set_custom_fields(&self, document_id: &DocumentID) -> reqwest::Result<()> {
        if self.custom_fields.custom_fields.is_empty() {
            return Ok(());
        }

        self.http_client
            .patch(self.url(&["api", "documents", document_id, ""]))
            .header("Authorization", format!("Token {}", self.token))
            .json(&self.custom_fields)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    async fn wait_for_processing(
        &self,
        upload_id: &str,
    ) -> Result<DocumentID, Box<dyn std::error::Error + Send + Sync>> {
        let start = Instant::now();

        while start.elapsed() < Duration::from_secs(30) {
            if let Some(id) = self.document_id(upload_id).await? {
                return Ok(id);
            }

            sleep(Duration::from_secs(1)).await;
        }

        Err("Timeout while waiting for processing.".into())
    }

    async fn document_id(
        &self,
        upload_id: &str,
    ) -> Result<Option<DocumentID>, Box<dyn std::error::Error + Send + Sync>> {
        let mut url = self.url(&["api", "tasks", ""]);
        url.set_query(Some(&format!("task_id={upload_id}")));

        let mut tasks = self
            .http_client
            .get(url)
            .header("Authorization", format!("Token {}", self.token))
            .send()
            .await?
            .error_for_status()?
            .json::<Vec<Task>>()
            .await?;

        if let Some(task) = tasks.pop() {
            match task.status {
                Some(TaskStatus::Started) | Some(TaskStatus::Pending) | None => Ok(None),
                Some(TaskStatus::Success) => Ok(task.related_document),
                Some(TaskStatus::Failure) => Err(format!(
                    "Processing failed {}",
                    task.result.as_ref().cloned().unwrap_or_default()
                )
                .into()),
                Some(TaskStatus::Other(status)) => {
                    Err(format!("Unknown processing status: {status}").into())
                }
            }
        } else {
            Err("Processing failed, task not found".into())
        }
    }

    fn url(&self, segments: &[&str]) -> Url {
        let mut url = self.endpoint.clone();

        {
            let mut path_segments = url.path_segments_mut().unwrap();
            path_segments.extend(segments);
        }

        url
    }
}

type DocumentID = String;

#[derive(Deserialize)]
struct Task {
    #[serde(default)]
    status: Option<TaskStatus>,

    #[serde(default)]
    result: Option<String>,

    #[serde(default)]
    related_document: Option<DocumentID>,
}

#[derive(Deserialize)]
#[serde(rename_all = "UPPERCASE")]
enum TaskStatus {
    Pending,
    Started,
    Failure,
    Success,
    #[serde(untagged)]
    Other(String),
}
