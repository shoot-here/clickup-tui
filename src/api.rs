use anyhow::{Context, Result};
use reqwest::header;
use serde::Deserialize;

const BASE: &str = "https://api.clickup.com/api/v2";

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
}

impl Client {
    pub fn new(token: String) -> Self {
        let mut headers = header::HeaderMap::new();
        let mut auth = header::HeaderValue::from_str(token.trim())
            .expect("api_token contains invalid header characters");
        auth.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth);
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("failed to build reqwest client");
        Self { http }
    }

    pub async fn workspaces(&self) -> Result<Vec<Workspace>> {
        #[derive(Deserialize)]
        struct Resp {
            teams: Vec<Workspace>,
        }
        let resp: Resp = self.get("/team").await?;
        Ok(resp.teams)
    }

    pub async fn spaces(&self, workspace_id: &str) -> Result<Vec<Space>> {
        #[derive(Deserialize)]
        struct Resp {
            spaces: Vec<Space>,
        }
        let resp: Resp = self
            .get(&format!("/team/{workspace_id}/space?archived=false"))
            .await?;
        Ok(resp.spaces)
    }

    pub async fn space_contents(&self, space_id: &str) -> Result<SpaceContents> {
        #[derive(Deserialize)]
        struct FolderResp {
            folders: Vec<Folder>,
        }
        #[derive(Deserialize)]
        struct ListResp {
            lists: Vec<ApiList>,
        }
        let folders_path = format!("/space/{space_id}/folder?archived=false");
        let folderless_path = format!("/space/{space_id}/list?archived=false");
        let (folders_res, folderless_res) = tokio::join!(
            self.get::<FolderResp>(&folders_path),
            self.get::<ListResp>(&folderless_path),
        );
        Ok(SpaceContents {
            folders: folders_res?.folders,
            folderless: folderless_res?.lists,
        })
    }

    pub async fn tasks(&self, list_id: &str) -> Result<Vec<Task>> {
        #[derive(Deserialize)]
        struct Resp {
            tasks: Vec<Task>,
        }
        let resp: Resp = self
            .get(&format!(
                "/list/{list_id}/task?archived=false&include_closed=false"
            ))
            .await?;
        Ok(resp.tasks)
    }

    pub async fn task(&self, task_id: &str) -> Result<Task> {
        self.get(&format!("/task/{task_id}")).await
    }

    pub async fn comments(&self, task_id: &str) -> Result<Vec<Comment>> {
        #[derive(Deserialize)]
        struct Resp {
            comments: Vec<Comment>,
        }
        let resp: Resp = self
            .get(&format!("/task/{task_id}/comment"))
            .await?;
        Ok(resp.comments)
    }

    pub async fn list_statuses(&self, list_id: &str) -> Result<Vec<TaskStatus>> {
        #[derive(Deserialize)]
        struct Resp {
            #[serde(default)]
            statuses: Vec<TaskStatus>,
        }
        let resp: Resp = self.get(&format!("/list/{list_id}")).await?;
        Ok(resp.statuses)
    }

    pub async fn create_task(
        &self,
        list_id: &str,
        body: serde_json::Value,
    ) -> Result<Task> {
        let url = format!("{BASE}/list/{list_id}/task");
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("create task {status}: {text}");
        }
        resp.json::<Task>()
            .await
            .with_context(|| format!("decode {url}"))
    }

    pub async fn update_task(
        &self,
        task_id: &str,
        body: serde_json::Value,
    ) -> Result<Task> {
        let url = format!("{BASE}/task/{task_id}");
        let resp = self
            .http
            .put(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("PUT {url}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("update task {status}: {text}");
        }
        resp.json::<Task>()
            .await
            .with_context(|| format!("decode {url}"))
    }

    pub async fn post_comment(&self, task_id: &str, text: &str) -> Result<()> {
        let url = format!("{BASE}/task/{task_id}/comment");
        let resp = self
            .http
            .post(&url)
            .json(&serde_json::json!({ "comment_text": text, "notify_all": false }))
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("post comment {status}: {body}");
        }
        Ok(())
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{BASE}{path}");
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET {url} → {status}: {body}");
        }
        resp.json::<T>()
            .await
            .with_context(|| format!("decode {url}"))
    }
}

// ── Types ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub members: Vec<Member>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Member {
    pub user: MemberUser,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MemberUser {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub username: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Space {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Folder {
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub lists: Vec<ApiList>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiList {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub task_count: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct ListEntry {
    pub id: String,
    pub name: String,
    pub task_count: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct SpaceContents {
    pub folders: Vec<Folder>,
    pub folderless: Vec<ApiList>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Task {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub status: Option<TaskStatus>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub text_content: Option<String>,
    #[serde(default)]
    pub assignees: Vec<Assignee>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub priority: Option<TaskPriority>,
    #[serde(default)]
    pub custom_fields: Vec<CustomField>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TaskPriority {
    pub priority: String,
    #[serde(default)]
    pub color: Option<String>,
}

/// (display name, ClickUp priority id, hex color). `None` id = clear priority.
pub const PRIORITY_OPTIONS: &[(&str, Option<u8>, &str)] = &[
    ("Urgent", Some(1), "#f50000"),
    ("High",   Some(2), "#ffcc00"),
    ("Normal", Some(3), "#6fddff"),
    ("Low",    Some(4), "#d8d8d8"),
    ("Clear",  None,    ""),
];

#[derive(Clone, Debug, Deserialize)]
pub struct CustomField {
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub type_config: serde_json::Value,
    #[serde(default)]
    pub value: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TaskStatus {
    pub status: String,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Assignee {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub username: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Comment {
    #[allow(dead_code)]
    pub id: String,
    pub comment_text: String,
    #[serde(default)]
    pub user: Option<CommentUser>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CommentUser {
    #[serde(default)]
    pub username: String,
}
