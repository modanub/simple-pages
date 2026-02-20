use axum::{
    extract::{Multipart, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use std::path::{Path as StdPath, PathBuf};

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::AppState;

#[derive(serde::Serialize)]
pub struct SiteInfo {
    pub username: String,
    pub disk_usage_bytes: u64,
    pub quota_bytes: u64,
    pub files: Vec<FileEntry>,
    pub site_url: String,
}

#[derive(serde::Serialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
}

pub async fn get_site_info(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<SiteInfo>, AppError> {
    let site_dir = state.config.sites_dir().join(&auth.username);
    let (files, total_size) = list_files_recursive(&site_dir)?;

    Ok(Json(SiteInfo {
        site_url: format!("/{}/", auth.username),
        username: auth.username,
        disk_usage_bytes: total_size,
        quota_bytes: state.config.disk_quota_bytes,
        files,
    }))
}

pub async fn upload_site(
    auth: AuthUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    let mut archive_data: Option<(String, Vec<u8>)> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("Multipart error: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name != "file" {
            continue;
        }

        let filename = field
            .file_name()
            .unwrap_or("upload")
            .to_string();

        let data = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(format!("Failed to read upload: {e}")))?;

        if data.len() as u64 > state.config.max_upload_bytes {
            return Err(AppError::PayloadTooLarge(format!(
                "File exceeds maximum upload size of {} MB",
                state.config.max_upload_bytes / (1024 * 1024)
            )));
        }

        archive_data = Some((filename, data.to_vec()));
        break;
    }

    let (filename, data) = archive_data
        .ok_or_else(|| AppError::BadRequest("No file uploaded".to_string()))?;

    // Extract to temp dir
    let temp_dir = tempfile::tempdir()
        .map_err(|e| AppError::Internal(format!("Failed to create temp dir: {e}")))?;

    if filename.ends_with(".zip") {
        extract_zip(&data, temp_dir.path())?;
    } else if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
        extract_tar_gz(&data, temp_dir.path())?;
    } else {
        return Err(AppError::BadRequest(
            "Unsupported format. Please upload a .zip or .tar.gz file".to_string(),
        ));
    }

    // Check extracted size against quota
    let (_, total_size) = list_files_recursive(temp_dir.path())?;
    if total_size > state.config.disk_quota_bytes {
        return Err(AppError::PayloadTooLarge(format!(
            "Extracted files ({:.1} MB) exceed disk quota of {} MB",
            total_size as f64 / (1024.0 * 1024.0),
            state.config.disk_quota_bytes / (1024 * 1024)
        )));
    }

    // Atomic replace: rename temp to site dir
    let site_dir = state.config.sites_dir().join(&auth.username);
    let old_dir = site_dir.with_extension("old");

    // Remove any leftover old dir
    let _ = std::fs::remove_dir_all(&old_dir);

    // Move current site to .old (if exists)
    if site_dir.exists() {
        std::fs::rename(&site_dir, &old_dir)?;
    }

    // Move extracted files to site dir
    std::fs::rename(temp_dir.path(), &site_dir).or_else(|_| {
        // Cross-device rename fallback: copy recursively
        copy_dir_recursive(temp_dir.path(), &site_dir)
    })?;

    // Clean up old dir
    let _ = std::fs::remove_dir_all(&old_dir);

    let body = serde_json::json!({
        "success": true,
        "site_url": format!("/{}/", auth.username),
        "disk_usage_bytes": total_size,
    });

    Ok((StatusCode::OK, Json(body)).into_response())
}

pub async fn delete_site(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let site_dir = state.config.sites_dir().join(&auth.username);
    if site_dir.exists() {
        std::fs::remove_dir_all(&site_dir)?;
        std::fs::create_dir_all(&site_dir)?;
    }
    Ok(Json(serde_json::json!({ "success": true })))
}

// Serve static files for user sites: /{username}/{path}
pub async fn serve_user_site(
    State(state): State<AppState>,
    Path((username, path)): Path<(String, String)>,
) -> Result<Response, AppError> {
    serve_file(&state, &username, &path).await
}

// Serve index for /{username}/
pub async fn serve_user_site_index(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> Result<Response, AppError> {
    serve_file(&state, &username, "index.html").await
}

async fn serve_file(state: &AppState, username: &str, path: &str) -> Result<Response, AppError> {
    // Validate username
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(AppError::NotFound("Not found".to_string()));
    }

    let sites_dir = state.config.sites_dir();
    let file_path = sites_dir.join(username).join(path);

    // Ensure the resolved path is within the user's site directory
    let canonical_sites = sites_dir
        .canonicalize()
        .unwrap_or_else(|_| sites_dir.clone());
    let canonical_file = file_path
        .canonicalize()
        .map_err(|_| AppError::NotFound("Not found".to_string()))?;

    if !canonical_file.starts_with(canonical_sites.join(username)) {
        return Err(AppError::Forbidden("Access denied".to_string()));
    }

    if canonical_file.is_dir() {
        // Try index.html
        let index = canonical_file.join("index.html");
        if index.exists() {
            return serve_static_file(&index).await;
        }
        return Err(AppError::NotFound("Not found".to_string()));
    }

    serve_static_file(&canonical_file).await
}

async fn serve_static_file(path: &StdPath) -> Result<Response, AppError> {
    let data = tokio::fs::read(path)
        .await
        .map_err(|_| AppError::NotFound("Not found".to_string()))?;

    let mime = mime_from_extension(
        path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or(""),
    );

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, mime)],
        data,
    )
        .into_response())
}

fn mime_from_extension(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "pdf" => "application/pdf",
        "xml" => "application/xml",
        "txt" => "text/plain; charset=utf-8",
        "md" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn extract_zip(data: &[u8], dest: &StdPath) -> Result<(), AppError> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| AppError::BadRequest(format!("Invalid zip file: {e}")))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| AppError::BadRequest(format!("Failed to read zip entry: {e}")))?;

        let raw_name = file.name().to_string();
        let entry_path = sanitize_archive_path(&raw_name)?;

        if file.is_dir() {
            std::fs::create_dir_all(dest.join(&entry_path))?;
            continue;
        }

        let out_path = dest.join(&entry_path);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut out_file = std::fs::File::create(&out_path)?;
        std::io::copy(&mut file, &mut out_file)?;
    }

    // If all files share a common top-level directory, flatten it
    flatten_single_root(dest)?;

    Ok(())
}

fn extract_tar_gz(data: &[u8], dest: &StdPath) -> Result<(), AppError> {
    let cursor = std::io::Cursor::new(data);
    let gz = flate2::read::GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(gz);

    for entry in archive
        .entries()
        .map_err(|e| AppError::BadRequest(format!("Invalid tar.gz: {e}")))?
    {
        let mut entry =
            entry.map_err(|e| AppError::BadRequest(format!("Failed to read tar entry: {e}")))?;

        let raw_path = entry
            .path()
            .map_err(|e| AppError::BadRequest(format!("Invalid path in archive: {e}")))?
            .to_string_lossy()
            .to_string();

        let entry_path = sanitize_archive_path(&raw_path)?;

        let out_path = dest.join(&entry_path);

        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else if entry.header().entry_type().is_file() {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out_file = std::fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut out_file)?;
        }
        // Skip symlinks and other special entries
    }

    flatten_single_root(dest)?;

    Ok(())
}

fn sanitize_archive_path(raw: &str) -> Result<PathBuf, AppError> {
    let path = StdPath::new(raw);

    // Reject absolute paths
    if path.has_root() {
        return Err(AppError::BadRequest(format!(
            "Absolute path in archive: {raw}"
        )));
    }

    // Build sanitized path, rejecting .. and dotfiles
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(c) => {
                let s = c.to_string_lossy();
                if s.starts_with('.') && s != "." {
                    return Err(AppError::BadRequest(format!(
                        "Hidden file in archive: {raw}"
                    )));
                }
                result.push(c);
            }
            std::path::Component::ParentDir => {
                return Err(AppError::BadRequest(format!(
                    "Path traversal in archive: {raw}"
                )));
            }
            _ => {} // skip CurDir, Prefix, RootDir
        }
    }

    if result.as_os_str().is_empty() {
        result.push(".");
    }

    Ok(result)
}

/// If all extracted contents sit inside a single top-level directory, move them up.
fn flatten_single_root(dir: &StdPath) -> Result<(), AppError> {
    let entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .collect();

    if entries.len() == 1 && entries[0].file_type().map(|t| t.is_dir()).unwrap_or(false) {
        let single_dir = entries[0].path();
        let temp_name = dir.join("__flatten_temp__");
        std::fs::rename(&single_dir, &temp_name)?;

        // Move all contents from the single dir up to parent
        for entry in std::fs::read_dir(&temp_name)? {
            let entry = entry?;
            std::fs::rename(entry.path(), dir.join(entry.file_name()))?;
        }
        std::fs::remove_dir(&temp_name)?;
    }

    Ok(())
}

fn list_files_recursive(dir: &StdPath) -> Result<(Vec<FileEntry>, u64), AppError> {
    let mut files = Vec::new();
    let mut total_size = 0u64;

    if !dir.exists() {
        return Ok((files, 0));
    }

    fn walk(
        base: &StdPath,
        current: &StdPath,
        files: &mut Vec<FileEntry>,
        total: &mut u64,
    ) -> Result<(), AppError> {
        if !current.is_dir() {
            return Ok(());
        }
        for entry in std::fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, files, total)?;
            } else {
                let size = entry.metadata()?.len();
                *total += size;
                let relative = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                files.push(FileEntry {
                    path: relative,
                    size,
                });
            }
        }
        Ok(())
    }

    walk(dir, dir, &mut files, &mut total_size)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((files, total_size))
}

fn copy_dir_recursive(src: &StdPath, dst: &StdPath) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
