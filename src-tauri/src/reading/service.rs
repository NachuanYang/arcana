use super::contract::{parse_and_validate_reading, ReadingResponse, ReadingValidationError};
use super::followup::{
    build_followup_prompt, parse_and_validate_followup, parse_and_validate_new_followup,
    FollowupResponse, FollowupValidationError,
};
use super::prompt::build_reading_prompt;
use super::tasks::ReadingTaskManager;
use crate::auth::inference::{
    followup_inference, reading_inference, FollowupInferenceRequest, InferenceError, ProxyMode,
};
use crate::auth::siwc::{store_for_app, ChatGptAuthManager, ChatGptConnectionStatus};
use crate::persistence::{SessionRepository, TarotSessionSnapshot, TarotTurn};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use zeroize::Zeroizing;

const READING_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadingProgressEvent<'a> {
    session_id: &'a str,
    request_id: &'a str,
    state: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartReadingRequest {
    session_id: String,
    request_id: String,
    model: String,
    proxy_mode: ProxyMode,
    consent_to_send: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartFollowupRequest {
    session_id: String,
    request_id: String,
    model: String,
    proxy_mode: ProxyMode,
    question: String,
    consent_to_send: bool,
}

struct ReadingTask<'a> {
    session_id: &'a str,
    request_id: &'a str,
    model: &'a str,
    proxy_mode: ProxyMode,
}

#[derive(Debug)]
enum FollowupFailure {
    LocalContext,
    Inference(InferenceError),
    Output(FollowupValidationError),
}

impl FollowupFailure {
    fn code(&self) -> &'static str {
        match self {
            Self::LocalContext => "local_context_invalid",
            Self::Output(error) => error.code(),
            Self::Inference(error) => match error {
                InferenceError::Unauthenticated => "unauthenticated",
                InferenceError::ExpiredOrRevoked => "authorization_expired",
                InferenceError::Timeout => "request_timeout",
                InferenceError::ProxyOrNetwork => "proxy_or_network_failure",
                InferenceError::ModelUnavailable => "model_unavailable",
                InferenceError::PlanLimit => "plan_limit",
                InferenceError::Cancelled => "cancelled",
                InferenceError::Server => "provider_server_failure",
                InferenceError::InvalidResponse => "invalid_response_stream",
                InferenceError::InvalidStructuredOutput(code) => code,
                InferenceError::IncompleteStream => "incomplete_response_stream",
                InferenceError::ProviderFailure { .. } => "provider_response_failed",
            },
        }
    }

    fn user_message(&self) -> String {
        match self {
            Self::LocalContext => "本地追问上下文未通过完整性校验，未生成可保存的答复。".into(),
            Self::Inference(error) => error.to_string(),
            Self::Output(error) => error.user_message().into(),
        }
    }

    fn turn_content(&self, stage: &str) -> String {
        json!({"stage":stage,"failureCode":self.code()}).to_string()
    }
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) async fn start_tarot_reading(
    app: AppHandle,
    auth: State<'_, Arc<ChatGptAuthManager>>,
    repository: State<'_, Arc<SessionRepository>>,
    tasks: State<'_, Arc<ReadingTaskManager>>,
    request: StartReadingRequest,
) -> Result<ReadingResponse, String> {
    if !request.consent_to_send {
        return Err("请先确认将本次问题和固定牌阵发送给OpenAI进行解读。".into());
    }
    if request.model.trim().is_empty() || request.model.len() > 128 {
        return Err("请从当前ChatGPT账号模型目录选择一个模型。".into());
    }
    let initial = repository
        .get_session(&request.session_id)?
        .ok_or_else(|| "找不到要解读的本地牌阵。".to_string())?;
    if initial.state == "READING_READY" {
        if let Some(turn) = initial.turns.iter().find(|turn| {
            turn.role == "assistant"
                && turn.request_id.as_deref() == Some(request.request_id.as_str())
                && turn.status == "COMPLETE"
        }) {
            return parse_and_validate_reading(&turn.content_json, &initial)
                .map_err(|_| "本地保存的解读未通过牌阵校验，无法继续。".to_string());
        }
        return Err("本次牌阵已经有完整解读。".into());
    }
    if !matches!(
        initial.state.as_str(),
        "REVEALED" | "READING_FAILED_RETRYABLE" | "READING_CANCELLED"
    ) {
        return Err("本次牌阵当前已有解读任务。".into());
    }

    let cancellation = tasks.begin(&request.session_id, &request.request_id)?;
    let result = run_reading(
        &app,
        auth.inner(),
        repository.inner(),
        ReadingTask {
            session_id: &request.session_id,
            request_id: &request.request_id,
            model: &request.model,
            proxy_mode: request.proxy_mode,
        },
        cancellation,
    )
    .await;
    tasks.finish(&request.session_id, &request.request_id);
    result
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) fn cancel_tarot_reading(
    session_id: String,
    request_id: String,
    tasks: State<'_, Arc<ReadingTaskManager>>,
) -> bool {
    tasks.cancel(&session_id, &request_id)
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) async fn start_tarot_followup(
    app: AppHandle,
    auth: State<'_, Arc<ChatGptAuthManager>>,
    repository: State<'_, Arc<SessionRepository>>,
    tasks: State<'_, Arc<ReadingTaskManager>>,
    request: StartFollowupRequest,
) -> Result<FollowupResponse, String> {
    if !request.consent_to_send {
        return Err("请先确认将本次追问和固定牌阵发送给OpenAI进行解读。".into());
    }
    if request.model.trim().is_empty() || request.model.len() > 128 {
        return Err("请从当前ChatGPT账号模型目录选择一个模型。".into());
    }
    let initial = repository
        .get_session(&request.session_id)?
        .ok_or_else(|| "找不到要继续追问的本地牌阵。".to_string())?;
    if initial.state == "READING_READY" {
        if let Some(answer) = initial.turns.iter().find(|turn| {
            turn.role == "assistant"
                && turn.request_id.as_deref() == Some(request.request_id.as_str())
                && turn.status == "COMPLETE"
                && turn.turn_order
                    > initial
                        .turns
                        .iter()
                        .filter(|item| item.role == "assistant")
                        .map(|item| item.turn_order)
                        .min()
                        .unwrap_or(i64::MAX)
        }) {
            let user = initial
                .turns
                .iter()
                .find(|turn| turn.role == "user" && turn.request_id == answer.request_id)
                .ok_or_else(|| "The saved follow-up question is missing".to_string())?;
            return parse_and_validate_followup(&answer.content_json, &initial, user);
        }
    }
    if initial.state != "READING_READY" {
        return Err("本次牌阵当前已有解读或追问任务。".into());
    }

    let cancellation = tasks.begin(&request.session_id, &request.request_id)?;
    let result = run_followup(
        &app,
        auth.inner(),
        repository.inner(),
        ReadingTask {
            session_id: &request.session_id,
            request_id: &request.request_id,
            model: &request.model,
            proxy_mode: request.proxy_mode,
        },
        &request.question,
        cancellation,
    )
    .await;
    tasks.finish(&request.session_id, &request.request_id);
    result
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) fn cancel_tarot_followup(
    session_id: String,
    request_id: String,
    tasks: State<'_, Arc<ReadingTaskManager>>,
) -> bool {
    tasks.cancel(&session_id, &request_id)
}

async fn run_reading(
    app: &AppHandle,
    auth: &ChatGptAuthManager,
    repository: &SessionRepository,
    task: ReadingTask<'_>,
    cancellation: tokio::sync::watch::Receiver<bool>,
) -> Result<ReadingResponse, String> {
    let ReadingTask {
        session_id,
        request_id,
        model,
        proxy_mode,
    } = task;
    let store = store_for_app(app)?;
    let access_token = Zeroizing::new(auth.access_token(&store).await?);
    let session = repository
        .get_session(session_id)?
        .ok_or_else(|| "找不到要解读的本地牌阵。".to_string())?;
    if !matches!(
        session.state.as_str(),
        "REVEALED" | "READING_FAILED_RETRYABLE" | "READING_CANCELLED"
    ) {
        return Err("本次牌阵当前已有解读任务。".into());
    }

    let turn = repository.begin_reading_turn(session_id, request_id, model)?;
    repository.transition_reading_turn(
        session_id,
        &turn.id,
        r#"{"stage":"streaming"}"#,
        "STREAMING",
    )?;
    emit_progress(app, session_id, request_id, "streaming");

    let inference = generate_reading_response(
        &session,
        model,
        proxy_mode,
        access_token.as_str(),
        cancellation,
    )
    .await;

    match inference {
        Ok(reading) => {
            let content = serde_json::to_string(&reading)
                .map_err(|_| "无法保存校验通过的解读内容。".to_string())?;
            repository.transition_reading_turn(session_id, &turn.id, &content, "COMPLETE")?;
            emit_progress(app, session_id, request_id, "completed");
            Ok(reading)
        }
        Err(error) => {
            let (status, state, message) = if error == InferenceError::Cancelled {
                ("CANCELLED", "cancelled", error.to_string())
            } else {
                ("FAILED_RETRYABLE", "failed", error.to_string())
            };
            let update = repository.transition_reading_turn(
                session_id,
                &turn.id,
                &json!({"stage":state}).to_string(),
                status,
            );
            if matches!(error, InferenceError::ExpiredOrRevoked) {
                let _ = auth
                    .clear_confirmed_invalid_tokens(&store_for_app(app)?)
                    .await;
                let _ = app.emit(
                    "auth://status-changed",
                    ChatGptConnectionStatus {
                        connected: false,
                        plan_usage_authorized: false,
                    },
                );
            }
            emit_progress(app, session_id, request_id, state);
            update.map_err(|db_error| format!("{message}；本地状态更新失败：{db_error}"))?;
            Err(message)
        }
    }
}

async fn generate_reading_response(
    session: &TarotSessionSnapshot,
    model: &str,
    proxy_mode: ProxyMode,
    access_token: &str,
    cancellation: tokio::sync::watch::Receiver<bool>,
) -> Result<ReadingResponse, InferenceError> {
    for attempt in 0..=1 {
        let prompt = build_reading_prompt(session, attempt == 1)
            .map_err(|_| InferenceError::InvalidResponse)?;
        #[cfg(all(test, feature = "live-m8-eval"))]
        m8_live_eval::record_provider_request_attempt();
        let result = reading_inference(
            access_token,
            model,
            &prompt.instructions,
            prompt.input,
            READING_TIMEOUT,
            proxy_mode,
            cancellation.clone(),
        )
        .await?;
        match map_reading_validation_error(parse_and_validate_reading(&result.output_text, session))
        {
            Ok(reading) => return Ok(reading),
            Err(_) if attempt == 0 => continue,
            Err(error) => return Err(error),
        }
    }
    Err(InferenceError::InvalidStructuredOutput(
        "invalid_output_schema",
    ))
}

pub(super) fn map_reading_validation_error<T>(
    result: Result<T, ReadingValidationError>,
) -> Result<T, InferenceError> {
    result.map_err(|error| InferenceError::InvalidStructuredOutput(error.code()))
}

async fn run_followup(
    app: &AppHandle,
    auth: &ChatGptAuthManager,
    repository: &SessionRepository,
    task: ReadingTask<'_>,
    question: &str,
    cancellation: tokio::sync::watch::Receiver<bool>,
) -> Result<FollowupResponse, String> {
    let ReadingTask {
        session_id,
        request_id,
        model,
        proxy_mode,
    } = task;
    let store = store_for_app(app)?;
    let access_token = Zeroizing::new(auth.access_token(&store).await?);
    let (user_turn, assistant_turn) =
        repository.begin_followup(session_id, request_id, question, model)?;
    if assistant_turn.status == "COMPLETE" {
        let session = repository
            .get_session(session_id)?
            .ok_or_else(|| "找不到要继续追问的本地牌阵。".to_string())?;
        return parse_and_validate_followup(&assistant_turn.content_json, &session, &user_turn);
    }
    repository.transition_followup_turn(
        session_id,
        &assistant_turn.id,
        r#"{"stage":"streaming"}"#,
        "STREAMING",
    )?;
    emit_progress(app, session_id, request_id, "followup_streaming");
    let session = repository
        .get_session(session_id)?
        .ok_or_else(|| "找不到要继续追问的本地牌阵。".to_string())?;

    let inference = generate_followup_response(
        &session,
        &user_turn,
        question,
        model,
        proxy_mode,
        access_token.as_str(),
        cancellation,
    )
    .await;

    match inference {
        Ok(response) => {
            let content = serde_json::to_string(&response)
                .map_err(|_| "无法保存校验通过的追问内容。".to_string())?;
            repository.transition_followup_turn(
                session_id,
                &assistant_turn.id,
                &content,
                "COMPLETE",
            )?;
            emit_progress(app, session_id, request_id, "followup_completed");
            Ok(response)
        }
        Err(error) => {
            let (status, state) = if matches!(
                &error,
                FollowupFailure::Inference(InferenceError::Cancelled)
            ) {
                ("CANCELLED", "followup_cancelled")
            } else {
                ("FAILED_RETRYABLE", "followup_failed")
            };
            let message = error.user_message();
            let update = repository.transition_followup_turn(
                session_id,
                &assistant_turn.id,
                &error.turn_content(state),
                status,
            );
            if matches!(
                &error,
                FollowupFailure::Inference(InferenceError::ExpiredOrRevoked)
            ) {
                let _ = auth
                    .clear_confirmed_invalid_tokens(&store_for_app(app)?)
                    .await;
                let _ = app.emit(
                    "auth://status-changed",
                    ChatGptConnectionStatus {
                        connected: false,
                        plan_usage_authorized: false,
                    },
                );
            }
            emit_progress(app, session_id, request_id, state);
            update.map_err(|db_error| format!("{message}；本地状态更新失败：{db_error}"))?;
            Err(message)
        }
    }
}

async fn generate_followup_response(
    session: &TarotSessionSnapshot,
    user_turn: &TarotTurn,
    question: &str,
    model: &str,
    proxy_mode: ProxyMode,
    access_token: &str,
    cancellation: tokio::sync::watch::Receiver<bool>,
) -> Result<FollowupResponse, FollowupFailure> {
    let prompt = build_followup_prompt(session, &user_turn.id, question)
        .map_err(|_| FollowupFailure::LocalContext)?;
    #[cfg(all(test, feature = "live-m8-eval"))]
    m8_live_eval::record_provider_request_attempt();
    let result = followup_inference(FollowupInferenceRequest {
        access_token,
        model,
        instructions: &prompt.instructions,
        input: prompt.input,
        session_id: &session.id,
        reply_to_turn_id: &user_turn.id,
        timeout: READING_TIMEOUT,
        proxy_mode,
        cancellation,
    })
    .await
    .map_err(FollowupFailure::Inference)?;
    parse_and_validate_new_followup(&result.output_text, session, user_turn)
        .map_err(FollowupFailure::Output)
}

#[cfg(all(test, feature = "live-m8-eval"))]
mod m8_live_eval;

fn emit_progress(app: &AppHandle, session_id: &str, request_id: &str, state: &str) {
    let _ = app.emit(
        "reading://progress",
        ReadingProgressEvent {
            session_id,
            request_id,
            state,
        },
    );
}

#[cfg(test)]
mod followup_failure_tests {
    use super::{FollowupFailure, InferenceError};
    use crate::reading::followup::FollowupValidationError;
    use serde_json::json;

    #[test]
    fn followup_failures_keep_only_safe_diagnostic_codes_in_turn_storage() {
        let cases = [
            (
                FollowupFailure::LocalContext,
                "followup_failed",
                "local_context_invalid",
            ),
            (
                FollowupFailure::Inference(InferenceError::InvalidResponse),
                "followup_failed",
                "invalid_response_stream",
            ),
            (
                FollowupFailure::Output(FollowupValidationError::MalformedSchema),
                "followup_failed",
                "invalid_output_schema",
            ),
            (
                FollowupFailure::Output(FollowupValidationError::CitationNotInSpread),
                "followup_failed",
                "citation_not_in_spread",
            ),
        ];

        for (failure, stage, code) in cases {
            let stored: serde_json::Value =
                serde_json::from_str(&failure.turn_content(stage)).unwrap();
            assert_eq!(stored, json!({"stage":stage,"failureCode":code}));
            assert!(!stored.to_string().contains("question"));
            assert!(!stored.to_string().contains("answer"));
        }
    }
}
