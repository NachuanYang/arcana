use super::{SessionRepository, TarotSessionSnapshot, TarotSessionSummary};
use std::sync::Arc;

#[tauri::command(rename_all = "camelCase")]
pub fn get_tarot_session(
    session_id: String,
    repository: tauri::State<'_, Arc<SessionRepository>>,
) -> Result<Option<TarotSessionSnapshot>, String> {
    repository.get_session(&session_id)
}

#[tauri::command]
pub fn list_tarot_sessions(
    repository: tauri::State<'_, Arc<SessionRepository>>,
) -> Result<Vec<TarotSessionSummary>, String> {
    repository.list_sessions()
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_tarot_session(
    session_id: String,
    repository: tauri::State<'_, Arc<SessionRepository>>,
) -> Result<bool, String> {
    repository.delete_session(&session_id)
}
