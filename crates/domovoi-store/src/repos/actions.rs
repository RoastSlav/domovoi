//! Action requests. A stored approval is loaded back with `HumanApproval::restore`, the only
//! place outside authentication that may build one (INV-1, ADR 0014).

use std::borrow::Cow;

use async_trait::async_trait;
use domovoi_core::ports::{ActionFilter, ActionRepo, Page, Paged, StoreError};
use domovoi_core::{
    ActionId, ActionRequest, ApprovalChannel, HumanApproval, Principal, StoredAction,
};

use crate::codec::{
    Cursor, fetch_limit, from_json, from_ms, from_name, hash32, json, ms, name, paginate, parse,
};
use crate::db::Store;
use crate::error::{corrupt, db};

macro_rules! action_columns {
    () => {
        "SELECT id, incident_id, host_id, skill, risk, args, plan_hash, status, \
    proposed_by_kind, proposed_by, approved_by, approved_channel, approved_at, expires_at, \
    created_at, updated_at, chat_token_hash FROM actions"
    };
}

#[derive(sqlx::FromRow)]
struct ActionRow {
    id: String,
    incident_id: String,
    host_id: String,
    skill: String,
    risk: String,
    args: String,
    plan_hash: String,
    status: String,
    proposed_by_kind: String,
    proposed_by: String,
    approved_by: Option<String>,
    approved_channel: Option<String>,
    approved_at: Option<i64>,
    expires_at: i64,
    created_at: i64,
    updated_at: i64,
    chat_token_hash: Option<Vec<u8>>,
}

impl ActionRow {
    fn into_action(self) -> Result<ActionRequest, StoreError> {
        let proposed_by = match self.proposed_by_kind.as_str() {
            "human" => Principal::Human(parse(&self.proposed_by, "proposer")?),
            "system" => Principal::System(Cow::Owned(self.proposed_by)),
            other => return Err(corrupt("proposer kind", other)),
        };
        let approved_by = match (self.approved_by, self.approved_channel, self.approved_at) {
            (Some(user), Some(channel), Some(at)) => Some(HumanApproval::restore(
                parse(&user, "approver")?,
                from_json::<ApprovalChannel>(&channel, "approval channel")?,
                from_ms(at)?,
            )),
            (None, None, None) => None,
            _ => return Err(corrupt("approval", "partially stored")),
        };
        Ok(ActionRequest::from_stored(StoredAction {
            id: parse(&self.id, "action id")?,
            incident: parse(&self.incident_id, "incident id")?,
            host: parse(&self.host_id, "host id")?,
            skill: from_name(&self.skill, "skill name")?,
            risk: from_name(&self.risk, "risk")?,
            args: from_json(&self.args, "action args")?,
            plan_hash: from_name(&self.plan_hash, "plan hash")?,
            status: from_json(&self.status, "action status")?,
            proposed_by,
            approved_by,
            expires_at: from_ms(self.expires_at)?,
            created_at: from_ms(self.created_at)?,
            updated_at: from_ms(self.updated_at)?,
            chat_token_hash: self
                .chat_token_hash
                .as_deref()
                .map(|h| hash32(h, "chat token hash"))
                .transpose()?,
        }))
    }
}

/// The values bound for every column after `id`, in [`COLUMNS`] order.
struct Bound {
    incident: String,
    host: String,
    skill: String,
    risk: String,
    args: String,
    plan_hash: String,
    status: String,
    proposed_kind: &'static str,
    proposed_by: String,
    approved_by: Option<String>,
    approved_channel: Option<String>,
    approved_at: Option<i64>,
    expires_at: i64,
    created_at: i64,
    updated_at: i64,
    chat_token_hash: Option<Vec<u8>>,
}

fn bound(action: &ActionRequest) -> Result<Bound, StoreError> {
    let (proposed_kind, proposed_by) = match action.proposed_by() {
        Principal::Human(user) => ("human", user.to_string()),
        Principal::System(component) => ("system", component.to_string()),
    };
    let approval = action.approved_by();
    Ok(Bound {
        incident: action.incident().to_string(),
        host: action.host().to_string(),
        skill: action.skill().to_string(),
        risk: name(&action.risk())?,
        args: json(action.args())?,
        plan_hash: name(&action.plan_hash())?,
        status: json(action.status())?,
        proposed_kind,
        proposed_by,
        approved_by: approval.map(|a| a.user().to_string()),
        approved_channel: approval.map(|a| json(a.channel())).transpose()?,
        approved_at: approval.map(|a| ms(a.at())),
        expires_at: ms(action.expires_at()),
        created_at: ms(action.created_at()),
        updated_at: ms(action.updated_at()),
        chat_token_hash: action.chat_token_hash().map(|h| h.to_vec()),
    })
}

macro_rules! bind_columns {
    ($query:expr, $b:expr) => {
        $query
            .bind($b.incident)
            .bind($b.host)
            .bind($b.skill)
            .bind($b.risk)
            .bind($b.args)
            .bind($b.plan_hash)
            .bind($b.status)
            .bind($b.proposed_kind)
            .bind($b.proposed_by)
            .bind($b.approved_by)
            .bind($b.approved_channel)
            .bind($b.approved_at)
            .bind($b.expires_at)
            .bind($b.created_at)
            .bind($b.updated_at)
            .bind($b.chat_token_hash)
    };
}

#[async_trait]
impl ActionRepo for Store {
    async fn insert(&self, action: &ActionRequest) -> Result<(), StoreError> {
        const SQL: &str = "INSERT INTO actions (id, incident_id, host_id, skill, risk, args, \
            plan_hash, status, proposed_by_kind, proposed_by, approved_by, approved_channel, \
            approved_at, expires_at, created_at, updated_at, chat_token_hash) \
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
        let b = bound(action)?;
        let query = sqlx::query(SQL).bind(action.id().to_string());
        bind_columns!(query, b)
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }

    async fn update(&self, action: &ActionRequest) -> Result<(), StoreError> {
        const SQL: &str = "UPDATE actions SET incident_id = ?, host_id = ?, skill = ?, \
            risk = ?, args = ?, plan_hash = ?, status = ?, proposed_by_kind = ?, \
            proposed_by = ?, approved_by = ?, approved_channel = ?, approved_at = ?, \
            expires_at = ?, created_at = ?, updated_at = ?, chat_token_hash = ? WHERE id = ?";
        let b = bound(action)?;
        let done = bind_columns!(sqlx::query(SQL), b)
            .bind(action.id().to_string())
            .execute(&self.write)
            .await
            .map_err(db)?;
        if done.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    async fn get(&self, id: ActionId) -> Result<Option<ActionRequest>, StoreError> {
        const SQL: &str = concat!(action_columns!(), " WHERE id = ?");
        let row: Option<ActionRow> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(ActionRow::into_action).transpose()
    }

    async fn list(
        &self,
        filter: &ActionFilter,
        page: &Page,
    ) -> Result<Paged<ActionRequest>, StoreError> {
        const SQL: &str = concat!(
            action_columns!(),
            " WHERE (?1 IS NULL OR incident_id = ?1) \
            AND (?2 IS NULL OR host_id = ?2) \
            AND (?3 IS NULL OR created_at < ?3 OR (created_at = ?3 AND id < ?4)) \
            ORDER BY created_at DESC, id DESC LIMIT ?5"
        );
        let (after_key, after_id) = Cursor::bind(page.cursor.as_deref())?;
        let limit = page.effective_limit();
        let rows: Vec<ActionRow> = sqlx::query_as(SQL)
            .bind(filter.incident.map(|i| i.to_string()))
            .bind(filter.host.map(|h| h.to_string()))
            .bind(after_key)
            .bind(after_id)
            .bind(fetch_limit(limit))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        let (rows, next_cursor) = paginate(rows, limit, |r| Cursor {
            key: r.created_at,
            id: r.id.clone(),
        });
        let items = rows
            .into_iter()
            .map(ActionRow::into_action)
            .collect::<Result<_, _>>()?;
        Ok(Paged { items, next_cursor })
    }

    async fn find_by_chat_token_hash(
        &self,
        hash: &[u8; 32],
    ) -> Result<Option<ActionRequest>, StoreError> {
        const SQL: &str = concat!(action_columns!(), " WHERE chat_token_hash = ?");
        let row: Option<ActionRow> = sqlx::query_as(SQL)
            .bind(&hash[..])
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(ActionRow::into_action).transpose()
    }
}
