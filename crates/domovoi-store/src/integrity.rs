//! The start-up integrity check.

use sqlx::sqlite::SqlitePool;

use crate::error::StoreOpenError;

/// Runs `PRAGMA quick_check` and fails unless it reports exactly one `ok`.
pub(crate) async fn quick_check(pool: &SqlitePool) -> Result<(), StoreOpenError> {
    let rows: Vec<(String,)> = sqlx::query_as("PRAGMA quick_check")
        .fetch_all(pool)
        .await
        .map_err(|e| StoreOpenError::Database(e.to_string()))?;
    let lines: Vec<String> = rows.into_iter().map(|(line,)| line).collect();
    judgment(&lines)
}

/// Decides whether the rows `PRAGMA quick_check` returned mean the database is sound.
fn judgment(lines: &[String]) -> Result<(), StoreOpenError> {
    if lines.len() == 1 && lines[0].eq_ignore_ascii_case("ok") {
        return Ok(());
    }
    let first: Vec<&str> = lines.iter().take(5).map(String::as_str).collect();
    Err(StoreOpenError::Integrity(first.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_single_ok_passes() {
        assert!(judgment(&["ok".to_owned()]).is_ok());
        assert!(judgment(&["OK".to_owned()]).is_ok());
        assert!(judgment(&["ok".to_owned(), "x".to_owned()]).is_err());
        assert!(judgment(&["*** in database main ***".to_owned()]).is_err());
        assert!(judgment(&[]).is_err());
    }
}
