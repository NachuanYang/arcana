use crate::tarot::deck::TAROT_DATASET_VERSION;
use crate::tarot::draw_engine::{validate_question, DrawnCard, RevealSnapshot};
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use uuid::Uuid;

const INITIAL_MIGRATION: &str = include_str!("../../migrations/0001_initial.sql");
const FOLLOWUP_MIGRATION: &str = include_str!("../../migrations/0002_followup_states.sql");
const CURRENT_SCHEMA_VERSION: i64 = 2;
const SPREAD_ID: &str = "three_present_obstacle_tendency";

type RepoResult<T> = Result<T, String>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TarotTurn {
    pub id: String,
    pub session_id: String,
    pub turn_order: i64,
    pub role: String,
    pub request_id: Option<String>,
    pub content_json: String,
    pub status: String,
    pub model_id: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TarotSessionSnapshot {
    pub id: String,
    pub question: String,
    pub spread_id: String,
    pub deck_version: String,
    pub state: String,
    pub created_at: String,
    pub revealed_at: String,
    pub updated_at: String,
    pub reading_stance: Option<String>,
    pub cards: Vec<DrawnCard>,
    pub turns: Vec<TarotTurn>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TarotSessionSummary {
    pub id: String,
    pub question: String,
    pub state: String,
    pub created_at: String,
    pub revealed_at: String,
    pub cards: Vec<DrawnCard>,
    pub turn_count: usize,
}

pub struct SessionRepository {
    connection: Mutex<Connection>,
}

impl SessionRepository {
    pub fn open(path: impl AsRef<Path>) -> RepoResult<Self> {
        let connection = Connection::open(path).map_err(db_error)?;
        Self::from_connection(connection)
    }

    pub fn in_memory() -> RepoResult<Self> {
        Self::from_connection(Connection::open_in_memory().map_err(db_error)?)
    }

    fn from_connection(connection: Connection) -> RepoResult<Self> {
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(db_error)?;
        migrate(&connection)?;
        let repository = Self {
            connection: Mutex::new(connection),
        };
        repository.recover_interrupted_work()?;
        Ok(repository)
    }

    pub fn schema_version(&self) -> RepoResult<i64> {
        self.lock()?
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(db_error)
    }

    pub fn persist_reveal(&self, snapshot: &RevealSnapshot) -> RepoResult<()> {
        validate_reveal(snapshot)?;
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        if let Some(existing) = load_session(&transaction, &snapshot.draw_id)? {
            if existing.question == snapshot.question
                && existing.deck_version == snapshot.dataset_version
                && existing.cards == snapshot.cards
            {
                transaction.commit().map_err(db_error)?;
                return Ok(());
            }
            return Err("This session already contains a different frozen spread".into());
        }

        let now = timestamp();
        transaction
            .execute(
                "INSERT INTO tarot_sessions
                 (id, question, spread_id, deck_version, state, created_at, revealed_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 'REVEALED', ?5, ?5, ?5)",
                params![
                    snapshot.draw_id,
                    snapshot.question,
                    SPREAD_ID,
                    snapshot.dataset_version,
                    now
                ],
            )
            .map_err(db_error)?;
        for drawn in &snapshot.cards {
            let card_snapshot_json = serde_json::to_string(drawn).map_err(json_error)?;
            transaction
                .execute(
                    "INSERT INTO tarot_drawn_cards
                     (session_id, slot, card_id, orientation, visual_order, card_snapshot_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        snapshot.draw_id,
                        drawn.slot,
                        drawn.deck_card_id,
                        drawn.orientation,
                        drawn.visual_order,
                        card_snapshot_json
                    ],
                )
                .map_err(db_error)?;
        }
        transaction.commit().map_err(db_error)
    }

    pub fn get_session(&self, session_id: &str) -> RepoResult<Option<TarotSessionSnapshot>> {
        let connection = self.lock()?;
        load_session(&connection, session_id)
    }

    pub fn list_sessions(&self) -> RepoResult<Vec<TarotSessionSummary>> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare("SELECT id FROM tarot_sessions ORDER BY created_at DESC, rowid DESC")
            .map_err(db_error)?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_error)?;
        ids.iter()
            .map(|id| {
                let session = load_session(&connection, id)?.ok_or_else(|| {
                    "Saved tarot session disappeared while loading history".to_string()
                })?;
                Ok(TarotSessionSummary {
                    id: session.id,
                    question: session.question,
                    state: session.state,
                    created_at: session.created_at,
                    revealed_at: session.revealed_at,
                    cards: session.cards,
                    turn_count: session.turns.len(),
                })
            })
            .collect()
    }

    pub fn delete_session(&self, session_id: &str) -> RepoResult<bool> {
        let connection = self.lock()?;
        connection
            .execute("DELETE FROM tarot_sessions WHERE id = ?1", [session_id])
            .map(|changed| changed == 1)
            .map_err(db_error)
    }

    pub fn append_turn(
        &self,
        session_id: &str,
        request_id: Option<&str>,
        role: &str,
        content_json: &str,
        status: &str,
        model_id: Option<&str>,
    ) -> RepoResult<TarotTurn> {
        if !matches!(role, "user" | "assistant") {
            return Err("Turn role must be user or assistant".into());
        }
        if !matches!(
            status,
            "PENDING" | "STREAMING" | "COMPLETE" | "FAILED_RETRYABLE" | "CANCELLED"
        ) {
            return Err("Turn status is invalid".into());
        }
        serde_json::from_str::<serde_json::Value>(content_json)
            .map_err(|_| "Turn content must be valid JSON".to_string())?;
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let session_exists = transaction
            .query_row(
                "SELECT 1 FROM tarot_sessions WHERE id = ?1",
                [session_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(db_error)?
            .is_some();
        if !session_exists {
            return Err("Tarot session does not exist".into());
        }

        if let Some(request_id) = request_id {
            if let Some(existing) = transaction
                .query_row(
                    "SELECT id, session_id, turn_order, role, request_id, content_json, status, model_id, created_at
                     FROM tarot_turns WHERE session_id = ?1 AND request_id = ?2 AND role = ?3",
                    params![session_id, request_id, role],
                    turn_from_row,
                )
                .optional()
                .map_err(db_error)?
            {
                if existing.content_json == content_json
                    && existing.status == status
                    && existing.model_id.as_deref() == model_id
                {
                    transaction.commit().map_err(db_error)?;
                    return Ok(existing);
                }
                return Err("This request already has a different saved turn".into());
            }
        }

        let turn_order = transaction
            .query_row(
                "SELECT COALESCE(MAX(turn_order), 0) + 1 FROM tarot_turns WHERE session_id = ?1",
                [session_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(db_error)?;
        let turn = TarotTurn {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            turn_order,
            role: role.into(),
            request_id: request_id.map(str::to_owned),
            content_json: content_json.into(),
            status: status.into(),
            model_id: model_id.map(str::to_owned),
            created_at: timestamp(),
        };
        transaction
            .execute(
                "INSERT INTO tarot_turns
                 (id, session_id, turn_order, role, request_id, content_json, status, model_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    turn.id,
                    turn.session_id,
                    turn.turn_order,
                    turn.role,
                    turn.request_id,
                    turn.content_json,
                    turn.status,
                    turn.model_id,
                    turn.created_at
                ],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE tarot_sessions SET updated_at = ?2 WHERE id = ?1",
                params![session_id, turn.created_at],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(turn)
    }

    pub fn begin_reading_turn(
        &self,
        session_id: &str,
        request_id: &str,
        model_id: &str,
    ) -> RepoResult<TarotTurn> {
        if request_id.trim().is_empty() || request_id.len() > 128 || model_id.trim().is_empty() {
            return Err("Reading request ID or model is invalid".into());
        }
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        if let Some(existing) = transaction
            .query_row(
                "SELECT id, session_id, turn_order, role, request_id, content_json, status, model_id, created_at
                 FROM tarot_turns WHERE session_id = ?1 AND request_id = ?2 AND role = 'assistant'",
                params![session_id, request_id],
                turn_from_row,
            )
            .optional()
            .map_err(db_error)?
        {
            if existing.status == "PENDING" && existing.model_id.as_deref() == Some(model_id) {
                transaction.commit().map_err(db_error)?;
                return Ok(existing);
            }
            return Err("This reading request ID has already been used".into());
        }
        let state = transaction
            .query_row(
                "SELECT state FROM tarot_sessions WHERE id = ?1",
                [session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| "Tarot session does not exist".to_string())?;
        if !matches!(
            state.as_str(),
            "REVEALED" | "READING_FAILED_RETRYABLE" | "READING_CANCELLED"
        ) {
            return Err("This tarot session is not ready for an initial reading".into());
        }
        let turn_order = transaction
            .query_row(
                "SELECT COALESCE(MAX(turn_order), 0) + 1 FROM tarot_turns WHERE session_id = ?1",
                [session_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(db_error)?;
        let now = timestamp();
        let turn = TarotTurn {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            turn_order,
            role: "assistant".into(),
            request_id: Some(request_id.into()),
            content_json: r#"{"stage":"pending"}"#.into(),
            status: "PENDING".into(),
            model_id: Some(model_id.into()),
            created_at: now.clone(),
        };
        transaction
            .execute(
                "INSERT INTO tarot_turns
                 (id, session_id, turn_order, role, request_id, content_json, status, model_id, created_at)
                 VALUES (?1, ?2, ?3, 'assistant', ?4, ?5, 'PENDING', ?6, ?7)",
                params![turn.id, session_id, turn_order, request_id, turn.content_json, model_id, now],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE tarot_sessions SET state = 'READING_PENDING', updated_at = ?2 WHERE id = ?1",
                params![session_id, turn.created_at],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(turn)
    }

    pub fn transition_reading_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        content_json: &str,
        status: &str,
    ) -> RepoResult<TarotTurn> {
        let session_state = match status {
            "STREAMING" => "READING_STREAMING",
            "COMPLETE" => "READING_READY",
            "FAILED_RETRYABLE" => "READING_FAILED_RETRYABLE",
            "CANCELLED" => "READING_CANCELLED",
            _ => return Err("Reading turn status is invalid".into()),
        };
        serde_json::from_str::<serde_json::Value>(content_json)
            .map_err(|_| "Turn content must be valid JSON".to_string())?;
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let existing = transaction
            .query_row(
                "SELECT id, session_id, turn_order, role, request_id, content_json, status, model_id, created_at
                 FROM tarot_turns WHERE session_id = ?1 AND id = ?2 AND role = 'assistant'",
                params![session_id, turn_id],
                turn_from_row,
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| "Reading turn does not exist in this session".to_string())?;
        let allowed = match existing.status.as_str() {
            "PENDING" => matches!(status, "STREAMING" | "FAILED_RETRYABLE" | "CANCELLED"),
            "STREAMING" => matches!(
                status,
                "STREAMING" | "COMPLETE" | "FAILED_RETRYABLE" | "CANCELLED"
            ),
            "COMPLETE" | "FAILED_RETRYABLE" | "CANCELLED" => {
                status == existing.status && content_json == existing.content_json
            }
            _ => false,
        };
        if !allowed {
            return Err("Reading turn cannot move backward or replace a completed response".into());
        }
        let now = timestamp();
        transaction
            .execute(
                "UPDATE tarot_turns SET content_json = ?3, status = ?4 WHERE session_id = ?1 AND id = ?2",
                params![session_id, turn_id, content_json, status],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE tarot_sessions
                 SET state = ?2, updated_at = ?3,
                     reading_stance = CASE WHEN ?4 = 'COMPLETE' THEN json_extract(?5, '$.stance') ELSE reading_stance END
                 WHERE id = ?1",
                params![session_id, session_state, now, status, content_json],
            )
            .map_err(db_error)?;
        let updated = transaction
            .query_row(
                "SELECT id, session_id, turn_order, role, request_id, content_json, status, model_id, created_at
                 FROM tarot_turns WHERE session_id = ?1 AND id = ?2",
                params![session_id, turn_id],
                turn_from_row,
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(updated)
    }

    pub fn begin_followup(
        &self,
        session_id: &str,
        request_id: &str,
        question: &str,
        model_id: &str,
    ) -> RepoResult<(TarotTurn, TarotTurn)> {
        let question = validate_question(question)?;
        if request_id.trim().is_empty()
            || request_id.len() > 128
            || model_id.trim().is_empty()
            || model_id.len() > 128
        {
            return Err("Follow-up request ID or model is invalid".into());
        }
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;

        let existing_user = transaction
            .query_row(
                "SELECT id, session_id, turn_order, role, request_id, content_json, status, model_id, created_at
                 FROM tarot_turns WHERE session_id = ?1 AND request_id = ?2 AND role = 'user'",
                params![session_id, request_id],
                turn_from_row,
            )
            .optional()
            .map_err(db_error)?;
        let existing_assistant = transaction
            .query_row(
                "SELECT id, session_id, turn_order, role, request_id, content_json, status, model_id, created_at
                 FROM tarot_turns WHERE session_id = ?1 AND request_id = ?2 AND role = 'assistant'
                   AND turn_order > (SELECT MIN(turn_order) FROM tarot_turns WHERE session_id = ?1 AND role = 'assistant')",
                params![session_id, request_id],
                turn_from_row,
            )
            .optional()
            .map_err(db_error)?;
        match (existing_user, existing_assistant) {
            (Some(user), Some(assistant)) => {
                let saved_question = serde_json::from_str::<serde_json::Value>(&user.content_json)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("question")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                    });
                if saved_question.as_deref() != Some(question.as_str()) {
                    return Err(
                        "This follow-up request ID was already used for different text".into(),
                    );
                }
                match assistant.status.as_str() {
                    "COMPLETE" => {
                        transaction.commit().map_err(db_error)?;
                        return Ok((user, assistant));
                    }
                    "PENDING" | "STREAMING" => {
                        return Err("This follow-up request is already running".into());
                    }
                    "FAILED_RETRYABLE" | "CANCELLED" => {
                        let state = transaction
                            .query_row(
                                "SELECT state FROM tarot_sessions WHERE id = ?1",
                                [session_id],
                                |row| row.get::<_, String>(0),
                            )
                            .optional()
                            .map_err(db_error)?
                            .ok_or_else(|| "Tarot session does not exist".to_string())?;
                        if state != "READING_READY" {
                            return Err(
                                "Another reading or follow-up is active for this session".into()
                            );
                        }
                        let now = timestamp();
                        transaction
                            .execute(
                                "UPDATE tarot_turns SET content_json = '{\"stage\":\"pending\"}', status = 'PENDING', model_id = ?3
                                 WHERE session_id = ?1 AND id = ?2",
                                params![session_id, assistant.id, model_id],
                            )
                            .map_err(db_error)?;
                        transaction
                            .execute(
                                "UPDATE tarot_sessions SET state = 'FOLLOWUP_PENDING', updated_at = ?2 WHERE id = ?1",
                                params![session_id, now],
                            )
                            .map_err(db_error)?;
                        let retried = transaction
                            .query_row(
                                "SELECT id, session_id, turn_order, role, request_id, content_json, status, model_id, created_at
                                 FROM tarot_turns WHERE session_id = ?1 AND id = ?2",
                                params![session_id, assistant.id],
                                turn_from_row,
                            )
                            .map_err(db_error)?;
                        transaction.commit().map_err(db_error)?;
                        return Ok((user, retried));
                    }
                    _ => return Err("Saved follow-up state is invalid".into()),
                }
            }
            (None, None) => {}
            _ => return Err("Saved follow-up turns are incomplete".into()),
        }

        let (state, stance) = transaction
            .query_row(
                "SELECT state, reading_stance FROM tarot_sessions WHERE id = ?1",
                [session_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| "Tarot session does not exist".to_string())?;
        if state != "READING_READY" {
            return Err("This tarot session is not ready for a follow-up".into());
        }
        let stance =
            stance.ok_or_else(|| "The first reading has no validated stance".to_string())?;
        if !matches!(
            stance.as_str(),
            "FAVORABLE" | "CAUTIOUS" | "UNFAVORABLE" | "INDETERMINATE"
        ) {
            return Err("The first reading stance is invalid".into());
        }
        let initial = transaction
            .query_row(
                "SELECT content_json FROM tarot_turns WHERE session_id = ?1 AND role = 'assistant' AND status = 'COMPLETE'
                 ORDER BY turn_order LIMIT 1",
                [session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| "The first reading is missing".to_string())?;
        if serde_json::from_str::<serde_json::Value>(&initial)
            .ok()
            .and_then(|value| {
                value
                    .get("stance")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .as_deref()
            != Some(stance.as_str())
        {
            return Err("The saved first reading does not match its stance".into());
        }
        let next_order: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(turn_order), 0) + 1 FROM tarot_turns WHERE session_id = ?1",
                [session_id],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        let now = timestamp();
        let user = TarotTurn {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            turn_order: next_order,
            role: "user".into(),
            request_id: Some(request_id.into()),
            content_json: serde_json::json!({"question":question}).to_string(),
            status: "COMPLETE".into(),
            model_id: None,
            created_at: now.clone(),
        };
        let assistant = TarotTurn {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            turn_order: next_order + 1,
            role: "assistant".into(),
            request_id: Some(request_id.into()),
            content_json: r#"{"stage":"pending"}"#.into(),
            status: "PENDING".into(),
            model_id: Some(model_id.into()),
            created_at: now.clone(),
        };
        for turn in [&user, &assistant] {
            transaction
                .execute(
                    "INSERT INTO tarot_turns
                     (id, session_id, turn_order, role, request_id, content_json, status, model_id, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![turn.id, turn.session_id, turn.turn_order, turn.role, turn.request_id, turn.content_json, turn.status, turn.model_id, turn.created_at],
                )
                .map_err(db_error)?;
        }
        transaction
            .execute(
                "UPDATE tarot_sessions SET state = 'FOLLOWUP_PENDING', updated_at = ?2 WHERE id = ?1",
                params![session_id, now],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok((user, assistant))
    }

    pub fn transition_followup_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        content_json: &str,
        status: &str,
    ) -> RepoResult<TarotTurn> {
        let session_state = match status {
            "STREAMING" => "FOLLOWUP_STREAMING",
            "COMPLETE" | "FAILED_RETRYABLE" | "CANCELLED" => "READING_READY",
            _ => return Err("Follow-up turn status is invalid".into()),
        };
        serde_json::from_str::<serde_json::Value>(content_json)
            .map_err(|_| "Turn content must be valid JSON".to_string())?;
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let existing = transaction
            .query_row(
                "SELECT id, session_id, turn_order, role, request_id, content_json, status, model_id, created_at
                 FROM tarot_turns WHERE session_id = ?1 AND id = ?2 AND role = 'assistant'
                   AND turn_order > (SELECT MIN(turn_order) FROM tarot_turns WHERE session_id = ?1 AND role = 'assistant')",
                params![session_id, turn_id],
                turn_from_row,
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| "Follow-up turn does not exist in this session".to_string())?;
        let allowed = match existing.status.as_str() {
            "PENDING" => matches!(status, "STREAMING" | "FAILED_RETRYABLE" | "CANCELLED"),
            "STREAMING" => matches!(
                status,
                "STREAMING" | "COMPLETE" | "FAILED_RETRYABLE" | "CANCELLED"
            ),
            "COMPLETE" | "FAILED_RETRYABLE" | "CANCELLED" => {
                status == existing.status && content_json == existing.content_json
            }
            _ => false,
        };
        if !allowed {
            return Err("Follow-up turn cannot move backward or replace a completed answer".into());
        }
        let now = timestamp();
        transaction
            .execute(
                "UPDATE tarot_turns SET content_json = ?3, status = ?4 WHERE session_id = ?1 AND id = ?2",
                params![session_id, turn_id, content_json, status],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE tarot_sessions SET state = ?2, updated_at = ?3 WHERE id = ?1",
                params![session_id, session_state, now],
            )
            .map_err(db_error)?;
        let updated = transaction
            .query_row(
                "SELECT id, session_id, turn_order, role, request_id, content_json, status, model_id, created_at
                 FROM tarot_turns WHERE session_id = ?1 AND id = ?2",
                params![session_id, turn_id],
                turn_from_row,
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(updated)
    }

    pub fn list_turns(&self, session_id: &str) -> RepoResult<Vec<TarotTurn>> {
        let connection = self.lock()?;
        load_turns(&connection, session_id)
    }

    pub fn update_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        content_json: &str,
        status: &str,
    ) -> RepoResult<TarotTurn> {
        if !matches!(
            status,
            "PENDING" | "STREAMING" | "COMPLETE" | "FAILED_RETRYABLE" | "CANCELLED"
        ) {
            return Err("Turn status is invalid".into());
        }
        serde_json::from_str::<serde_json::Value>(content_json)
            .map_err(|_| "Turn content must be valid JSON".to_string())?;
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let existing = transaction
            .query_row(
                "SELECT id, session_id, turn_order, role, request_id, content_json, status, model_id, created_at
                 FROM tarot_turns WHERE session_id = ?1 AND id = ?2",
                params![session_id, turn_id],
                turn_from_row,
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| "Tarot turn does not exist in this session".to_string())?;
        let transition_allowed = match existing.status.as_str() {
            "PENDING" => matches!(
                status,
                "PENDING" | "STREAMING" | "FAILED_RETRYABLE" | "CANCELLED"
            ),
            "STREAMING" => matches!(
                status,
                "STREAMING" | "COMPLETE" | "FAILED_RETRYABLE" | "CANCELLED"
            ),
            "FAILED_RETRYABLE" => matches!(status, "PENDING" | "FAILED_RETRYABLE" | "CANCELLED"),
            "COMPLETE" | "CANCELLED" => {
                status == existing.status && content_json == existing.content_json
            }
            _ => false,
        };
        if !transition_allowed {
            return Err("Turn status cannot move backward or replace a completed response".into());
        }
        let now = timestamp();
        transaction
            .execute(
                "UPDATE tarot_turns SET content_json = ?3, status = ?4 WHERE session_id = ?1 AND id = ?2",
                params![session_id, turn_id, content_json, status],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE tarot_sessions SET updated_at = ?2 WHERE id = ?1",
                params![session_id, now],
            )
            .map_err(db_error)?;
        let updated = transaction
            .query_row(
                "SELECT id, session_id, turn_order, role, request_id, content_json, status, model_id, created_at
                 FROM tarot_turns WHERE session_id = ?1 AND id = ?2",
                params![session_id, turn_id],
                turn_from_row,
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(updated)
    }

    pub fn set_session_state(&self, session_id: &str, state: &str) -> RepoResult<()> {
        if !valid_session_state(state) {
            return Err("Session state is invalid".into());
        }
        let connection = self.lock()?;
        let changed = connection
            .execute(
                "UPDATE tarot_sessions SET state = ?2, updated_at = ?3 WHERE id = ?1",
                params![session_id, state, timestamp()],
            )
            .map_err(db_error)?;
        if changed != 1 {
            return Err("Tarot session does not exist".into());
        }
        Ok(())
    }

    pub fn recover_interrupted_work(&self) -> RepoResult<usize> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE tarot_turns SET status = 'FAILED_RETRYABLE'
                 WHERE status IN ('PENDING', 'STREAMING')
                   AND session_id IN (
                     SELECT id FROM tarot_sessions
                     WHERE state IN ('READING_PENDING', 'READING_STREAMING', 'FOLLOWUP_PENDING', 'FOLLOWUP_STREAMING')
                   )",
                [],
            )
            .map_err(db_error)?;
        let now = timestamp();
        let changed = transaction
            .execute(
                "UPDATE tarot_sessions
                 SET state = CASE WHEN state IN ('FOLLOWUP_PENDING', 'FOLLOWUP_STREAMING')
                                  THEN 'READING_READY' ELSE 'READING_FAILED_RETRYABLE' END,
                     updated_at = ?1
                 WHERE state IN ('READING_PENDING', 'READING_STREAMING', 'FOLLOWUP_PENDING', 'FOLLOWUP_STREAMING')",
                [&now],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(changed)
    }

    fn lock(&self) -> RepoResult<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| "Local history database is unavailable".to_string())
    }

    #[cfg(test)]
    pub(super) fn fail_tendency_insert_for_test(&self) -> RepoResult<()> {
        self.lock()?
            .execute_batch(
                "CREATE TRIGGER fail_tendency BEFORE INSERT ON tarot_drawn_cards
                 WHEN NEW.slot = 'tendency' BEGIN SELECT RAISE(FAIL, 'injected insert failure'); END;",
            )
            .map_err(db_error)
    }

    #[cfg(test)]
    pub(super) fn card_count_for_test(&self, session_id: &str) -> RepoResult<usize> {
        self.lock()?
            .query_row(
                "SELECT COUNT(*) FROM tarot_drawn_cards WHERE session_id = ?1",
                [session_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count as usize)
            .map_err(db_error)
    }

    #[cfg(test)]
    pub(super) fn child_record_counts_for_test(
        &self,
        session_id: &str,
    ) -> RepoResult<(usize, usize)> {
        let connection = self.lock()?;
        let cards = connection
            .query_row(
                "SELECT COUNT(*) FROM tarot_drawn_cards WHERE session_id = ?1",
                [session_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count as usize)
            .map_err(db_error)?;
        let turns = connection
            .query_row(
                "SELECT COUNT(*) FROM tarot_turns WHERE session_id = ?1",
                [session_id],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count as usize)
            .map_err(db_error)?;
        Ok((cards, turns))
    }

    #[cfg(test)]
    pub(super) fn corrupt_card_json_for_test(&self, session_id: &str) -> RepoResult<()> {
        let connection = self.lock()?;
        connection
            .pragma_update(None, "ignore_check_constraints", "ON")
            .map_err(db_error)?;
        connection.execute(
            "UPDATE tarot_drawn_cards SET card_snapshot_json = '{' WHERE session_id = ?1 AND visual_order = 0",
            [session_id],
        ).map_err(db_error)?;
        connection
            .pragma_update(None, "ignore_check_constraints", "OFF")
            .map_err(db_error)
    }

    #[cfg(test)]
    pub(super) fn corrupt_turn_json_for_test(&self, session_id: &str) -> RepoResult<()> {
        let connection = self.lock()?;
        connection
            .pragma_update(None, "ignore_check_constraints", "ON")
            .map_err(db_error)?;
        connection
            .execute(
                "UPDATE tarot_turns SET content_json = '{' WHERE session_id = ?1",
                [session_id],
            )
            .map_err(db_error)?;
        connection
            .pragma_update(None, "ignore_check_constraints", "OFF")
            .map_err(db_error)
    }

    #[cfg(test)]
    pub(super) fn check_schema_constraints_for_test(
        &self,
        session_id: &str,
    ) -> RepoResult<Vec<bool>> {
        let connection = self.lock()?;
        let state = connection
            .execute(
                "UPDATE tarot_sessions SET state = 'NOT_A_STATE' WHERE id = ?1",
                [session_id],
            )
            .is_err();
        let slot = connection.execute(
            "UPDATE tarot_drawn_cards SET slot = 'not_a_slot' WHERE session_id = ?1 AND visual_order = 0",
            [session_id],
        ).is_err();
        let orientation = connection.execute(
            "UPDATE tarot_drawn_cards SET orientation = 'sideways' WHERE session_id = ?1 AND visual_order = 0",
            [session_id],
        ).is_err();
        let json = connection.execute(
            "UPDATE tarot_drawn_cards SET card_snapshot_json = '{' WHERE session_id = ?1 AND visual_order = 0",
            [session_id],
        ).is_err();
        let foreign_key = connection
            .execute(
                "INSERT INTO tarot_drawn_cards
             (session_id, slot, card_id, orientation, visual_order, card_snapshot_json)
             VALUES ('missing-session', 'present', 'major_x', 'upright', 0, '{}')",
                [],
            )
            .is_err();
        Ok(vec![state, slot, orientation, json, foreign_key])
    }
}

fn migrate(connection: &Connection) -> RepoResult<()> {
    let mut current: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(db_error)?;
    if current > CURRENT_SCHEMA_VERSION {
        return Err(format!(
            "Database schema version {current} is newer than this app supports"
        ));
    }
    if current == 0 {
        let transaction = connection.unchecked_transaction().map_err(db_error)?;
        transaction
            .execute_batch(INITIAL_MIGRATION)
            .map_err(db_error)?;
        transaction
            .pragma_update(None, "user_version", 1)
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        current = 1;
    }
    if current == 1 {
        connection
            .pragma_update(None, "foreign_keys", "OFF")
            .map_err(db_error)?;
        if let Err(error) = connection.execute_batch(FOLLOWUP_MIGRATION) {
            let _ = connection.execute_batch("ROLLBACK;");
            let _ = connection.pragma_update(None, "foreign_keys", "ON");
            return Err(db_error(error));
        }
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(db_error)?;
        current = 2;
    }
    if current != CURRENT_SCHEMA_VERSION {
        return Err(format!("Database schema version {current} is unsupported"));
    }
    Ok(())
}

fn load_session(
    connection: &Connection,
    session_id: &str,
) -> RepoResult<Option<TarotSessionSnapshot>> {
    let header = connection
        .query_row(
            "SELECT id, question, spread_id, deck_version, state, created_at, revealed_at, updated_at, reading_stance
             FROM tarot_sessions WHERE id = ?1",
            [session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            },
        )
        .optional()
        .map_err(db_error)?;
    let Some((
        id,
        question,
        spread_id,
        deck_version,
        state,
        created_at,
        revealed_at,
        updated_at,
        reading_stance,
    )) = header
    else {
        return Ok(None);
    };
    let cards = load_cards(connection, session_id)?;
    if cards.len() != 3 {
        return Err("Saved tarot session does not contain exactly three frozen cards".into());
    }
    if spread_id != SPREAD_ID || !valid_session_state(&state) {
        return Err("Saved tarot session contains an invalid state or spread".into());
    }
    validate_reveal(&RevealSnapshot {
        draw_id: id.clone(),
        dataset_version: deck_version.clone(),
        question: question.clone(),
        cards: cards.clone(),
    })
    .map_err(|_| "Saved tarot session contains an invalid frozen spread".to_string())?;
    let turns = load_turns(connection, session_id)?;
    Ok(Some(TarotSessionSnapshot {
        id,
        question,
        spread_id,
        deck_version,
        state,
        created_at,
        revealed_at,
        updated_at,
        reading_stance,
        cards,
        turns,
    }))
}

fn load_cards(connection: &Connection, session_id: &str) -> RepoResult<Vec<DrawnCard>> {
    let mut statement = connection
        .prepare(
            "SELECT card_snapshot_json FROM tarot_drawn_cards WHERE session_id = ?1 ORDER BY visual_order ASC",
        )
        .map_err(db_error)?;
    let encoded = statement
        .query_map([session_id], |row| row.get::<_, String>(0))
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    encoded
        .iter()
        .map(|json| {
            serde_json::from_str(json).map_err(|_| "Saved tarot card snapshot is malformed".into())
        })
        .collect()
}

fn load_turns(connection: &Connection, session_id: &str) -> RepoResult<Vec<TarotTurn>> {
    let mut statement = connection
        .prepare(
            "SELECT id, session_id, turn_order, role, request_id, content_json, status, model_id, created_at
             FROM tarot_turns WHERE session_id = ?1 ORDER BY turn_order ASC",
        )
        .map_err(db_error)?;
    let turns = statement
        .query_map([session_id], turn_from_row)
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    Ok(turns)
}

fn turn_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TarotTurn> {
    let turn = TarotTurn {
        id: row.get(0)?,
        session_id: row.get(1)?,
        turn_order: row.get(2)?,
        role: row.get(3)?,
        request_id: row.get(4)?,
        content_json: row.get(5)?,
        status: row.get(6)?,
        model_id: row.get(7)?,
        created_at: row.get(8)?,
    };
    if !matches!(turn.role.as_str(), "user" | "assistant")
        || !matches!(
            turn.status.as_str(),
            "PENDING" | "STREAMING" | "COMPLETE" | "FAILED_RETRYABLE" | "CANCELLED"
        )
        || serde_json::from_str::<serde_json::Value>(&turn.content_json).is_err()
    {
        return Err(rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Saved tarot turn is malformed or invalid",
            )),
        ));
    }
    Ok(turn)
}

fn validate_reveal(snapshot: &RevealSnapshot) -> RepoResult<()> {
    if validate_question(&snapshot.question)? != snapshot.question {
        return Err("Saved question must be normalized".into());
    }
    if snapshot.dataset_version != TAROT_DATASET_VERSION {
        return Err("Reveal uses an unverified tarot dataset".into());
    }
    if snapshot.cards.len() != 3 {
        return Err("A revealed session must contain exactly three cards".into());
    }
    let expected_slots = ["present", "obstacle", "tendency"];
    let mut card_ids = std::collections::HashSet::new();
    for (index, drawn) in snapshot.cards.iter().enumerate() {
        if drawn.slot != expected_slots[index]
            || drawn.visual_order != index as u8
            || drawn.deck_card_id != drawn.card.id
            || !matches!(drawn.orientation.as_str(), "upright" | "reversed")
            || !card_ids.insert(drawn.deck_card_id.as_str())
        {
            return Err("Reveal contains an invalid or duplicate frozen card".into());
        }
    }
    Ok(())
}

fn valid_session_state(state: &str) -> bool {
    matches!(
        state,
        "REVEALED"
            | "READING_PENDING"
            | "READING_STREAMING"
            | "READING_READY"
            | "READING_FAILED_RETRYABLE"
            | "READING_CANCELLED"
            | "FOLLOWUP_PENDING"
            | "FOLLOWUP_STREAMING"
            | "ARCHIVED"
    )
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn db_error(error: rusqlite::Error) -> String {
    format!("Local history database error: {error}")
}

fn json_error(error: serde_json::Error) -> String {
    format!("Local history snapshot could not be encoded: {error}")
}
