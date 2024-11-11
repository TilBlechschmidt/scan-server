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
use tokio::{task::JoinSet, time::sleep};
use uuid::Uuid;

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
            http_client: reqwest::ClientBuilder::new()
                .danger_accept_invalid_certs(true)
                .build()
                .unwrap(), // Client::new(),
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
        let file_name_prefix = format!("EpicPrinter-{title}");

        let file = Part::stream(body)
            .file_name(format!("{file_name_prefix}.pdf"))
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
            .json::<Uuid>()
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
            match client
                .wait_for_processing(&upload_id, &file_name_prefix)
                .await
            {
                Ok(_) => debug!("{title}\tProcessing complete"),
                Err(err) => error!("{title}\t{err}"),
            }
        });

        Ok(())
    }

    async fn set_document_attributes(
        &self,
        document_id: DocumentID,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.custom_fields.custom_fields.is_empty() {
            return Ok(());
        }

        self.http_client
            .patch(self.url(&["api", "documents", &document_id, ""]))
            .header("Authorization", format!("Token {}", self.token))
            .json(&self.custom_fields)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    async fn wait_for_processing(
        &self,
        upload_id: &TaskID,
        file_name_prefix: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        debug!("{upload_id:?} Waiting for processing");
        let result = self
            .wait_for_task(upload_id, Duration::from_secs(5), Duration::from_secs(300))
            .await?;

        match result {
            TaskResult::Success(document_id) => self.set_document_attributes(document_id).await,
            TaskResult::Failure(message) => Err(message.into()),

            TaskResult::Split => {
                let mut tasks = JoinSet::new();

                debug!("{upload_id:?} Handling split processing");
                for task in self.find_related_tasks(file_name_prefix).await? {
                    if task.uuid == *upload_id {
                        continue;
                    }

                    debug!("{upload_id:?} Spawning split task {}", task.uuid);
                    let client = self.clone();
                    tasks.spawn(async move { client.wait_for_split_processing(&task.uuid).await });
                }

                tasks
                    .join_all()
                    .await
                    .into_iter()
                    .reduce(|acc, result| acc.or(result))
                    .expect("No child tasks for split upload")
            }
        }
    }

    async fn wait_for_split_processing(
        &self,
        split_id: &TaskID,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let result = self
            .wait_for_task(split_id, Duration::from_secs(5), Duration::from_secs(300))
            .await?;

        match result {
            TaskResult::Success(document_id) => self.set_document_attributes(document_id).await,
            TaskResult::Failure(message) => Err(message.into()),
            TaskResult::Split => unreachable!("Split of a split should not be possible"),
        }
    }

    async fn wait_for_task(
        &self,
        id: &TaskID,
        interval: Duration,
        timeout: Duration,
    ) -> Result<TaskResult, Box<dyn std::error::Error + Send + Sync>> {
        let start = Instant::now();

        debug!("{id:?} Waiting for task");

        while start.elapsed() < timeout {
            let task = self.fetch_task(id).await?;

            if let Some(result) = task.result() {
                debug!("{id:?} Task completed");
                return Ok(result);
            }

            // TODO Use exponential backoff
            sleep(interval).await;
        }

        Err("Timeout while waiting for processing.".into())
    }

    async fn find_related_tasks(
        &self,
        file_name_prefix: &str,
    ) -> Result<impl Iterator<Item = Task> + '_, Box<dyn std::error::Error + Send + Sync>> {
        let tasks = self.fetch_tasks().await?;
        let file_name_prefix = file_name_prefix.to_string();

        Ok(tasks.into_iter().filter(move |task| {
            task.file_name
                .as_ref()
                .map(|n| n.starts_with(&file_name_prefix))
                .unwrap_or_default()
        }))
    }

    async fn fetch_task(
        &self,
        id: &TaskID,
    ) -> Result<Task, Box<dyn std::error::Error + Send + Sync>> {
        let mut url = self.url(&["api", "tasks", ""]);
        url.set_query(Some(&format!("task_id={id}")));

        Ok(self
            .http_client
            .get(url)
            .header("Authorization", format!("Token {}", self.token))
            .send()
            .await?
            .error_for_status()?
            .json::<Vec<Task>>()
            .await?
            .pop()
            .ok_or(Box::<dyn std::error::Error + Send + Sync>::from(
                "Received empty response to task query",
            ))?)
    }

    async fn fetch_tasks(&self) -> Result<Vec<Task>, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.url(&["api", "tasks", ""]);

        Ok(self
            .http_client
            .get(url)
            .header("Authorization", format!("Token {}", self.token))
            .send()
            .await?
            .error_for_status()?
            .json::<Vec<Task>>()
            .await?)
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
type TaskID = Uuid;

#[derive(Deserialize)]
struct Task {
    #[serde(rename = "task_id")]
    uuid: Uuid,

    #[serde(default, rename = "task_file_name")]
    file_name: Option<String>,

    #[serde(default)]
    status: Option<TaskStatus>,

    #[serde(default, rename = "result")]
    message: Option<String>,

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

#[derive(PartialEq, Eq)]
enum TaskResult {
    /// Processing failed with the given reason
    Failure(String),

    /// Input file has been split into multiple child tasks
    Split,

    /// Finished and created the given document
    Success(DocumentID),
}

impl Task {
    fn result(&self) -> Option<TaskResult> {
        let message = self.message.as_ref().cloned().unwrap_or_default();

        let result = match self.status.as_ref()? {
            // Running or about to be
            TaskStatus::Started | TaskStatus::Pending => return None,

            // Input has been split into new tasks
            TaskStatus::Success if message.contains("splitting complete") => TaskResult::Split,

            // Finished successfully with a document ID
            TaskStatus::Success if self.related_document.is_some() => {
                TaskResult::Success(self.related_document.clone().unwrap())
            }

            // Upstream failure
            TaskStatus::Failure => TaskResult::Failure(message),

            // Missing document ID on finished task
            TaskStatus::Success => panic!("Missing document ID on task"),

            // Unknown task state which we should handle
            TaskStatus::Other(status) => panic!("Unknown processing status: {status}"),
        };

        Some(result)
    }
}
