//! Settings, users and web sessions.

use async_trait::async_trait;
use domovoi_core::ports::{SessionRepo, SettingsRepo, StoreError, UserRepo};
use domovoi_core::{Session, User, UserId};
use jiff::Timestamp;
use serde_json::Value;

use crate::codec::{from_json, from_ms, hash32, json, ms, parse, uint};
use crate::db::Store;
use crate::error::db;

#[async_trait]
impl SettingsRepo for Store {
    async fn get(&self, key: &str) -> Result<Option<Value>, StoreError> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(|(value,)| from_json(&value, "setting")).transpose()
    }

    async fn set(&self, key: &str, value: &Value) -> Result<(), StoreError> {
        const SQL: &str = "INSERT INTO settings (key, value) VALUES (?, ?) \
            ON CONFLICT (key) DO UPDATE SET value = excluded.value";
        sqlx::query(SQL)
            .bind(key)
            .bind(json(value)?)
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }

    async fn set_many(&self, writes: &[(String, Option<Value>)]) -> Result<(), StoreError> {
        const SET: &str = "INSERT INTO settings (key, value) VALUES (?, ?) \
            ON CONFLICT (key) DO UPDATE SET value = excluded.value";
        let mut tx = self.write.begin().await.map_err(db)?;
        for (key, value) in writes {
            match value {
                Some(value) => sqlx::query(SET).bind(key).bind(json(value)?),
                None => sqlx::query("DELETE FROM settings WHERE key = ?").bind(key),
            }
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        }
        tx.commit().await.map_err(db)
    }

    async fn list_prefix(&self, prefix: &str) -> Result<Vec<(String, Value)>, StoreError> {
        // `substr` rather than LIKE, so `_` and `%` in a prefix match literally.
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT key, value FROM settings WHERE substr(key, 1, length(?1)) = ?1 ORDER BY key",
        )
        .bind(prefix)
        .fetch_all(&self.read)
        .await
        .map_err(db)?;
        rows.into_iter()
            .map(|(key, value)| Ok((key, from_json(&value, "setting")?)))
            .collect()
    }
}

fn user(row: (String, String, String, i64)) -> Result<User, StoreError> {
    let (id, username, password_hash, created_at) = row;
    Ok(User {
        id: parse(&id, "user id")?,
        username,
        password_hash,
        created_at: from_ms(created_at)?,
    })
}

macro_rules! user_columns {
    () => {
        "SELECT id, username, password_hash, created_at FROM users"
    };
}

#[async_trait]
impl UserRepo for Store {
    async fn create(&self, new: &User) -> Result<(), StoreError> {
        const SQL: &str =
            "INSERT INTO users (id, username, password_hash, created_at) VALUES (?, ?, ?, ?)";
        sqlx::query(SQL)
            .bind(new.id.to_string())
            .bind(&new.username)
            .bind(&new.password_hash)
            .bind(ms(new.created_at))
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }

    async fn get(&self, id: UserId) -> Result<Option<User>, StoreError> {
        const SQL: &str = concat!(user_columns!(), " WHERE id = ?");
        let row = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(user).transpose()
    }

    async fn get_by_username(&self, username: &str) -> Result<Option<User>, StoreError> {
        // `=` compares TEXT byte for byte, so usernames are case-sensitive.
        const SQL: &str = concat!(user_columns!(), " WHERE username = ?");
        let row = sqlx::query_as(SQL)
            .bind(username)
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(user).transpose()
    }

    async fn set_password_hash(&self, id: UserId, password_hash: &str) -> Result<(), StoreError> {
        let done = sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
            .bind(password_hash)
            .bind(id.to_string())
            .execute(&self.write)
            .await
            .map_err(db)?;
        if done.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    async fn count(&self) -> Result<u64, StoreError> {
        let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM users")
            .fetch_one(&self.read)
            .await
            .map_err(db)?;
        Ok(uint(count))
    }

    async fn first_user(&self) -> Result<Option<User>, StoreError> {
        const SQL: &str = concat!(user_columns!(), " ORDER BY created_at, id LIMIT 1");
        let row = sqlx::query_as(SQL)
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(user).transpose()
    }
}

#[async_trait]
impl SessionRepo for Store {
    async fn create(&self, session: &Session) -> Result<(), StoreError> {
        const SQL: &str = "INSERT INTO sessions (token_hash, user_id, csrf_hash, created_at, \
            expires_at) VALUES (?, ?, ?, ?, ?)";
        sqlx::query(SQL)
            .bind(&session.token_hash[..])
            .bind(session.user.to_string())
            .bind(&session.csrf_hash[..])
            .bind(ms(session.created_at))
            .bind(ms(session.expires_at))
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }

    async fn get(
        &self,
        token_hash: &[u8; 32],
        now: Timestamp,
    ) -> Result<Option<Session>, StoreError> {
        const SQL: &str = "SELECT user_id, csrf_hash, created_at, expires_at FROM sessions \
            WHERE token_hash = ? AND expires_at > ?";
        let row: Option<(String, Vec<u8>, i64, i64)> = sqlx::query_as(SQL)
            .bind(&token_hash[..])
            .bind(ms(now))
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(|(user, csrf, created, expires)| {
            Ok(Session {
                token_hash: *token_hash,
                user: parse(&user, "user id")?,
                csrf_hash: hash32(&csrf, "csrf hash")?,
                created_at: from_ms(created)?,
                expires_at: from_ms(expires)?,
            })
        })
        .transpose()
    }

    async fn delete(&self, token_hash: &[u8; 32]) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(&token_hash[..])
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }

    async fn delete_for_user(&self, owner: UserId) -> Result<u64, StoreError> {
        let done = sqlx::query("DELETE FROM sessions WHERE user_id = ?")
            .bind(owner.to_string())
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(done.rows_affected())
    }

    async fn extend(&self, token_hash: &[u8; 32], expires_at: Timestamp) -> Result<(), StoreError> {
        sqlx::query("UPDATE sessions SET expires_at = ? WHERE token_hash = ?")
            .bind(ms(expires_at))
            .bind(&token_hash[..])
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }

    async fn purge_expired(&self, now: Timestamp) -> Result<u64, StoreError> {
        let done = sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
            .bind(ms(now))
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(done.rows_affected())
    }
}
