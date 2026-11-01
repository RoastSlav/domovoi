//! `POST /api/v1/notes/import`: notes from a folder of Markdown files, uploaded as
//! `multipart/form-data`.

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{Multipart, State};
use domovoi_core::{Note, NoteId};
use utoipa::ToSchema;

use super::dto_admin::{ImportResultDto, SkippedFileDto};
use super::error::{ApiError, ErrorBody};
use super::notes::{MAX_BODY_BYTES, MAX_TITLE_CHARS, host_id};
use super::{ApiState, Authed};

/// The most files one import takes.
pub const MAX_FILES: usize = 200;

/// The largest file an import takes, in bytes; the same as the largest note body.
pub const MAX_FILE_BYTES: usize = MAX_BODY_BYTES;

/// The largest import request: every file at its limit, plus room for the form itself.
pub const MAX_REQUEST_BYTES: usize = MAX_FILES * MAX_FILE_BYTES + (1 << 20);

/// The form an import posts. Documents the request; the handler reads it field by field.
#[derive(Debug, ToSchema)]
pub struct NotesImportForm {
    /// The Markdown files, one form field each. Only `.md` files are imported.
    #[schema(value_type = Vec<String>, format = Binary)]
    pub files: Vec<Vec<u8>>,
    /// The host the notes are about, by name; absent for global notes.
    pub host: Option<String>,
}

/// One uploaded file.
struct Upload {
    name: String,
    bytes: Vec<u8>,
}

/// The last path segment of an uploaded file name; folder uploads send relative paths.
#[must_use]
pub fn base_name(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

/// The note title for a file: its first `# ` heading, otherwise the file name without
/// `.md`, cut to the longest title.
#[must_use]
pub fn title_of(file_name: &str, body: &str) -> String {
    let heading = body
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("# "))
        .map(str::trim)
        .filter(|h| !h.is_empty());
    let title = heading.unwrap_or_else(|| {
        let base = base_name(file_name);
        base.strip_suffix(".md").unwrap_or(base).trim()
    });
    title.chars().take(MAX_TITLE_CHARS).collect()
}

/// The body of `file` if it can become a note, otherwise why not.
pub fn check_file(name: &str, bytes: &[u8]) -> Result<String, String> {
    if !base_name(name).to_ascii_lowercase().ends_with(".md") {
        return Err("not a .md file".to_owned());
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err(format!("larger than {} KiB", MAX_FILE_BYTES / 1024));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "not UTF-8 text".to_owned())?;
    if text.trim().is_empty() {
        return Err("empty".to_owned());
    }
    Ok(text.to_owned())
}

fn bad_form(err: &axum::extract::multipart::MultipartError) -> ApiError {
    ApiError::BadRequest(format!("the upload could not be read: {}", err.body_text()))
}

/// Reads the form: the optional host and up to [`MAX_FILES`] files.
async fn read_form(mut form: Multipart) -> Result<(Option<String>, Vec<Upload>), ApiError> {
    let mut host = None;
    let mut files = Vec::new();
    while let Some(field) = form.next_field().await.map_err(|e| bad_form(&e))? {
        match field.name() {
            Some("host") => {
                let text = field.text().await.map_err(|e| bad_form(&e))?;
                host = Some(text.trim().to_owned()).filter(|h| !h.is_empty());
            }
            Some("files") => {
                if files.len() == MAX_FILES {
                    return Err(ApiError::BadRequest(format!(
                        "at most {MAX_FILES} files per import"
                    )));
                }
                let name = field.file_name().unwrap_or("unnamed").to_owned();
                let bytes = field.bytes().await.map_err(|e| bad_form(&e))?;
                files.push(Upload {
                    name,
                    bytes: bytes.to_vec(),
                });
            }
            _ => {}
        }
    }
    Ok((host, files))
}

/// Imports Markdown files as notes. Each note's title is its first `# ` heading or the
/// file name. Files that are not `.md`, too large, not UTF-8, empty, or titled like an
/// existing note in the same scope are skipped, so importing a folder twice adds nothing.
#[utoipa::path(
    post, path = "/api/v1/notes/import", operation_id = "import_notes", tag = "notes",
    request_body(content = NotesImportForm, content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "What was imported and skipped", body = ImportResultDto),
        (status = 400, description = "Too many files or an unreadable upload", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn import(
    State(state): State<ApiState>,
    _auth: Authed,
    form: Multipart,
) -> Result<Json<ImportResultDto>, ApiError> {
    let (host, files) = read_form(form).await?;
    let host = host_id(&state, host.as_deref()).await?;
    let mut titles: BTreeSet<String> = state
        .deps
        .notes
        .list(host)
        .await?
        .into_iter()
        .map(|n| n.title)
        .collect();
    let mut created = 0u32;
    let mut skipped_files = Vec::new();
    for file in files {
        let checked = check_file(&file.name, &file.bytes).and_then(|body| {
            let title = title_of(&file.name, &body);
            if title.is_empty() {
                Err("no title".to_owned())
            } else if titles.contains(&title) {
                Err(format!("a note titled \"{title}\" already exists"))
            } else {
                Ok((title, body))
            }
        });
        match checked {
            Ok((title, body)) => {
                let note = Note {
                    id: NoteId::new(),
                    host,
                    title: title.clone(),
                    body_markdown: body,
                    tags: Vec::new(),
                    updated_at: state.deps.clock.now(),
                };
                state.deps.notes.upsert(&note).await?;
                titles.insert(title);
                created += 1;
            }
            Err(reason) => skipped_files.push(SkippedFileDto {
                name: file.name,
                reason,
            }),
        }
    }
    if created > 0 {
        tracing::info!(created, skipped = skipped_files.len(), "notes imported");
    }
    Ok(Json(ImportResultDto {
        created,
        skipped: u32::try_from(skipped_files.len()).unwrap_or(u32::MAX),
        skipped_files,
    }))
}

#[cfg(test)]
mod tests {
    use super::{check_file, title_of};

    #[test]
    fn titles_come_from_the_first_heading_or_the_file_name() {
        assert_eq!(title_of("x.md", "intro\n# Backups\n## More"), "Backups");
        assert_eq!(title_of("docs/nas/zfs-pools.md", "no heading"), "zfs-pools");
        assert_eq!(title_of("a\\b\\c.md", "##  not h1"), "c");
        assert_eq!(
            title_of("t.md", &format!("# {}", "x".repeat(300))).len(),
            200
        );
    }

    #[test]
    fn files_are_checked_for_kind_size_encoding_and_content() {
        assert!(check_file("a.md", b"# hi").is_ok());
        assert!(check_file("A.MD", b"text").is_ok());
        assert_eq!(check_file("a.txt", b"x").unwrap_err(), "not a .md file");
        assert_eq!(
            check_file("a.md", &[0xff, 0xfe]).unwrap_err(),
            "not UTF-8 text"
        );
        assert_eq!(check_file("a.md", b"  \n").unwrap_err(), "empty");
        assert!(
            check_file("a.md", &vec![b'a'; 300 * 1024])
                .unwrap_err()
                .contains("KiB")
        );
    }
}
