//! Google Drive API v3: the parts the other services use (search, export, trash, delete);
//! the full file API comes with gdrive.
use serde::Serialize;

use crate::{ApiError, Session};

pub const BASE: &str = "https://www.googleapis.com/drive/v3/files";
pub const DOC_MIME: &str = "application/vnd.google-apps.document";

/// A string literal for a Drive `q` expression: `\\` and `'` escaped (pure).
pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The `q` for documents of `mime`, optionally whose name contains `name` (pure).
pub fn query(mime: &str, name: Option<&str>) -> String {
    let mut q = format!("mimeType = {} and trashed = false", quote(mime));
    if let Some(name) = name.filter(|n| !n.is_empty()) {
        q.push_str(&format!(" and name contains {}", quote(name)));
    }
    q
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    pub modified_time: Option<String>,
}

pub const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
pub const FIELDS: &str = "id, name, mimeType, size, modifiedTime, webViewLink, parents";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileMeta {
    pub id: String,
    pub name: String,
    pub mime_type: Option<String>,
    pub size: Option<String>,
    pub modified_time: Option<String>,
    pub web_view_link: Option<String>,
    pub parents: Vec<String>,
}

impl FileMeta {
    pub fn is_folder(&self) -> bool {
        self.mime_type.as_deref() == Some(FOLDER_MIME)
    }
    /// Docs, Sheets, Slides...: exported, not downloaded.
    pub fn is_native(&self) -> bool {
        self.mime_type.as_deref().is_some_and(|m| m.starts_with("application/vnd.google-apps.") && m != FOLDER_MIME)
    }
}

/// An API file resource (`None` without id or name) (pure).
pub fn file_meta(v: &serde_json::Value) -> Option<FileMeta> {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    Some(FileMeta {
        id: s("id")?,
        name: s("name")?,
        mime_type: s("mimeType"),
        size: s("size"),
        modified_time: s("modifiedTime"),
        web_view_link: s("webViewLink"),
        parents: v.get("parents").and_then(|p| p.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
    })
}

/// The `q` listing a folder's children (or everything) (pure).
pub fn children_query(parent: Option<&str>) -> String {
    match parent {
        Some(p) => format!("{} in parents and trashed = false", quote(p)),
        None => "trashed = false".to_string(),
    }
}

/// The `q` for names containing `query`, optionally of one MIME type (pure).
pub fn name_query(query: &str, mime: Option<&str>) -> String {
    let mut q = format!("name contains {} and trashed = false", quote(query));
    if let Some(mime) = mime {
        q.push_str(&format!(" and mimeType = {}", quote(mime)));
    }
    q
}

/// A multipart/related upload body: JSON metadata, then the media (pure).
pub fn multipart(metadata: &serde_json::Value, mime: &str, content: &[u8], boundary: &str) -> Vec<u8> {
    let mut body = format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata}\r\n--{boundary}\r\nContent-Type: {mime}\r\n\r\n").into_bytes();
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Quota {
    pub limit: Option<u64>,
    pub usage: u64,
    pub usage_in_drive: u64,
    pub usage_in_drive_trash: u64,
}

/// One entry of a folder walk: its path below the folder, as Drive names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: Vec<String>,
    pub meta: FileMeta,
}

pub struct Drive {
    pub session: Session,
}

impl Drive {
    /// Newest first; at most 100.
    pub async fn search(&self, mime: &str, name: Option<&str>, max: usize) -> Result<Vec<DriveFile>, ApiError> {
        let mut url = self.session.url(&[]);
        url.query_pairs_mut()
            .append_pair("q", &query(mime, name))
            .append_pair("fields", "files(id, name, modifiedTime)")
            .append_pair("pageSize", &max.min(100).to_string())
            .append_pair("orderBy", "modifiedTime desc");
        let response: serde_json::Value = self.session.get(&url).await?;
        let files = response.get("files").and_then(|f| f.as_array()).cloned().unwrap_or_default();
        Ok(files
            .iter()
            .filter_map(|f| {
                Some(DriveFile {
                    id: f.get("id")?.as_str()?.to_string(),
                    name: f.get("name")?.as_str()?.to_string(),
                    modified_time: f.get("modifiedTime").and_then(|v| v.as_str()).map(str::to_string),
                })
            })
            .collect())
    }

    /// files.export of a native Workspace file to `mime` (Drive caps it at 10 MB).
    pub async fn export(&self, id: &str, mime: &str) -> Result<Vec<u8>, ApiError> {
        let mut url = self.session.url(&[id, "export"]);
        url.query_pairs_mut().append_pair("mimeType", mime);
        self.session.bytes(&url).await
    }

    pub async fn trash(&self, id: &str) -> Result<(), ApiError> {
        let _: serde_json::Value = self.session.send(reqwest::Method::PATCH, &self.session.url(&[id]), &serde_json::json!({ "trashed": true })).await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<(), ApiError> {
        self.session.delete(&self.session.url(&[id])).await
    }

    /// The upload and about endpoints are siblings of the files endpoint (`.../drive/v3/files`
    /// -> `.../upload/drive/v3/files`, `.../drive/v3/about`).
    pub fn sibling(&self, which: &str) -> String {
        let base = self.session.base.trim_end_matches('/');
        let root = base.strip_suffix("/drive/v3/files").unwrap_or(base);
        match which {
            "upload" => format!("{root}/upload/drive/v3/files"),
            other => format!("{root}/drive/v3/{other}"),
        }
}

    pub async fn list(&self, parent: Option<&str>, max: usize) -> Result<Vec<FileMeta>, ApiError> {
        let mut url = self.session.url(&[]);
        url.query_pairs_mut()
            .append_pair("q", &children_query(parent))
            .append_pair("fields", &format!("nextPageToken, files({FIELDS})"))
            .append_pair("pageSize", &max.clamp(1, 1000).to_string())
            .append_pair("orderBy", "modifiedTime desc");
        Ok(self.session.paged(&url, "files", Some(max)).await?.iter().filter_map(file_meta).collect())
}
    pub async fn find(&self, query: &str, mime: Option<&str>, max: usize) -> Result<Vec<FileMeta>, ApiError> {
        let mut url = self.session.url(&[]);
        url.query_pairs_mut()
            .append_pair("q", &name_query(query, mime))
            .append_pair("fields", &format!("nextPageToken, files({FIELDS})"))
            .append_pair("pageSize", &max.clamp(1, 1000).to_string())
            .append_pair("orderBy", "modifiedTime desc");
        Ok(self.session.paged(&url, "files", Some(max)).await?.iter().filter_map(file_meta).collect())
}
    pub async fn metadata(&self, id: &str) -> Result<FileMeta, ApiError> {
        let mut url = self.session.url(&[id]);
        url.query_pairs_mut().append_pair("fields", FIELDS);
        let raw: serde_json::Value = self.session.get(&url).await?;
        file_meta(&raw).ok_or_else(|| ApiError::Failed { api: url.path().to_string(), status: 200, body: "file without id or name".into() })
}
    pub async fn download(&self, id: &str) -> Result<Vec<u8>, ApiError> {
        let mut url = self.session.url(&[id]);
        url.query_pairs_mut().append_pair("alt", "media");
        self.session.bytes(&url).await
}
    /// Raw bytes, or an export for native types; folders are an error.
    pub async fn fetch(&self, meta: &FileMeta, export_mime: &str) -> Result<Vec<u8>, ApiError> {
        if meta.is_folder() {
            return Err(ApiError::Failed { api: meta.id.clone(), status: 0, body: format!("{} is a folder; download it as a tree", meta.name) });
        }
        if meta.is_native() { self.export(&meta.id, export_mime).await } else { self.download(&meta.id).await }
}
    /// One multipart request: name, parent and content together (no orphan on failure).
    pub async fn upload(&self, name: &str, mime: &str, content: &[u8], parent: Option<&str>) -> Result<FileMeta, ApiError> {
        let mut metadata = serde_json::json!({ "name": name });
        if let Some(p) = parent {
            metadata["parents"] = serde_json::json!([p]);
        }
        let mut boundary = format!("rafa-gworkspace-{}", content.len());
        while content.windows(boundary.len()).any(|w| w == boundary.as_bytes()) {
            boundary.push('x');
        }
        let mut url = url::Url::parse(&self.sibling("upload")).map_err(|e| ApiError::Http(e.to_string()))?;
        url.query_pairs_mut().append_pair("uploadType", "multipart").append_pair("fields", FIELDS);
        let raw: serde_json::Value =
            self.session.post_bytes(&url, &format!("multipart/related; boundary={boundary}"), multipart(&metadata, mime, content, &boundary)).await?;
        file_meta(&raw).ok_or_else(|| ApiError::Failed { api: url.path().to_string(), status: 200, body: "upload without id or name".into() })
}
    pub async fn create_folder(&self, name: &str, parent: Option<&str>) -> Result<FileMeta, ApiError> {
        let mut body = serde_json::json!({ "name": name, "mimeType": FOLDER_MIME });
        if let Some(p) = parent {
            body["parents"] = serde_json::json!([p]);
        }
        let mut url = self.session.url(&[]);
        url.query_pairs_mut().append_pair("fields", FIELDS);
        let raw: serde_json::Value = self.session.send(reqwest::Method::POST, &url, &body).await?;
        file_meta(&raw).ok_or_else(|| ApiError::Failed { api: url.path().to_string(), status: 200, body: "folder without id or name".into() })
}
    /// Move under `parent`, out of every current parent.
    pub async fn move_to(&self, id: &str, parent: &str) -> Result<FileMeta, ApiError> {
        let current = self.metadata(id).await?;
        let mut url = self.session.url(&[id]);
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("addParents", parent);
            if !current.parents.is_empty() {
                q.append_pair("removeParents", &current.parents.join(","));
            }
            q.append_pair("fields", FIELDS);
        }
        let raw: serde_json::Value = self.session.send(reqwest::Method::PATCH, &url, &serde_json::json!({})).await?;
        file_meta(&raw).ok_or_else(|| ApiError::Failed { api: url.path().to_string(), status: 200, body: "moved file without id or name".into() })
}
    pub async fn quota(&self) -> Result<Quota, ApiError> {
        let mut url = url::Url::parse(&self.sibling("about")).map_err(|e| ApiError::Http(e.to_string()))?;
        url.query_pairs_mut().append_pair("fields", "storageQuota");
        let raw: serde_json::Value = self.session.get(&url).await?;
        let n = |k: &str| raw.pointer(&format!("/storageQuota/{k}")).and_then(|v| v.as_str()).and_then(|s| s.parse::<u64>().ok());
        Ok(Quota { limit: n("limit"), usage: n("usage").unwrap_or(0), usage_in_drive: n("usageInDrive").unwrap_or(0), usage_in_drive_trash: n("usageInDriveTrash").unwrap_or(0) })
}
    /// A `/`-separated folder path from the root to its folder id; ambiguity is an error.
    pub async fn resolve_path(&self, path: &str) -> Result<String, ApiError> {
        let mut current = "root".to_string();
        for segment in path.split('/').filter(|s| !s.is_empty()) {
            let mut url = self.session.url(&[]);
            let q = format!("name = {} and {} in parents and mimeType = {} and trashed = false", quote(segment), quote(&current), quote(FOLDER_MIME));
            url.query_pairs_mut().append_pair("q", &q).append_pair("fields", "files(id, name)");
            let found: serde_json::Value = self.session.get(&url).await?;
            let ids: Vec<String> = found.get("files").and_then(|f| f.as_array()).map(|a| a.iter().filter_map(|x| x.get("id")?.as_str().map(str::to_string)).collect()).unwrap_or_default();
            current = match ids.as_slice() {
                [one] => one.clone(),
                [] => return Err(ApiError::Failed { api: "path".into(), status: 404, body: format!("no folder {segment:?} under {current}") }),
                many => return Err(ApiError::Failed { api: "path".into(), status: 409, body: format!("{segment:?} under {current} is ambiguous: {}", many.join(", ")) }),
            };
        }
        Ok(current)
}
    /// Every descendant, breadth first, paging each folder, each folder visited once.
    pub async fn walk(&self, folder: &str) -> Result<Vec<TreeEntry>, ApiError> {
        let mut out = Vec::new();
        let mut visited = std::collections::HashSet::from([folder.to_string()]);
        let mut queue = std::collections::VecDeque::from([(folder.to_string(), Vec::<String>::new())]);
        while let Some((id, base)) = queue.pop_front() {
            let mut url = self.session.url(&[]);
            url.query_pairs_mut()
                .append_pair("q", &children_query(Some(&id)))
                .append_pair("fields", &format!("nextPageToken, files({FIELDS})"))
                .append_pair("pageSize", "1000");
            for child in self.session.paged(&url, "files", None).await?.iter().filter_map(file_meta) {
                let mut path = base.clone();
                path.push(child.name.clone());
                if child.is_folder() && visited.insert(child.id.clone()) {
                    queue.push_back((child.id.clone(), path.clone()));
                }
                out.push(TreeEntry { path, meta: child });
            }
        }
        Ok(out)
}
}
