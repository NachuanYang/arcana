use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::{fmt, time::Duration};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::watch;
use zeroize::Zeroize;

const OFFICIAL_API_BASE: &str = "https://api.openai.com/v1";
const MAX_CATALOG_BYTES: usize = 2 * 1024 * 1024;
const MAX_STREAM_BYTES: usize = 16 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AvailableModel {
    pub slug: String,
    pub display_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InferenceResult {
    pub output_text: String,
    pub response_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InferenceError {
    Unauthenticated,
    ExpiredOrRevoked,
    Timeout,
    ProxyOrNetwork,
    ModelUnavailable,
    PlanLimit,
    Cancelled,
    Server,
    InvalidResponse,
    InvalidStructuredOutput(&'static str),
    IncompleteStream,
    ProviderFailure {
        code: Option<String>,
        response_id: Option<String>,
    },
}

impl fmt::Display for InferenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::ProviderFailure { code, response_id } = self {
            let message = provider_failure_message(code.as_deref());
            return write!(
                formatter,
                "{message} 错误码：{}；响应编号：{}",
                code.as_deref().unwrap_or("未提供"),
                response_id.as_deref().unwrap_or("未提供")
            );
        }

        let message = match self {
            Self::Unauthenticated => "ChatGPT账号未授权访问此资源；请检查计划用量权限或重新连接。",
            Self::ExpiredOrRevoked => "ChatGPT授权已过期或被撤销，请重新连接账号。",
            Self::Timeout => "ChatGPT请求超时；可重试，或检查网络与系统代理。",
            Self::ProxyOrNetwork => "无法连接OpenAI服务；请检查网络/系统代理，或切换为直连后重试。",
            Self::ModelUnavailable => "所选模型当前不可用，请刷新模型目录后重试。",
            Self::PlanLimit => "ChatGPT计划用量或请求频率受限，请稍后重试并检查账号用量。",
            Self::Cancelled => "请求已取消。",
            Self::Server => "OpenAI服务暂时无法完成请求，请稍后重试。",
            Self::InvalidResponse => "OpenAI返回了无法识别的响应，未将其视为成功。",
            Self::InvalidStructuredOutput(_) => {
                "OpenAI返回的解读未通过固定牌阵校验，未保存为成功结果；可以重试。"
            }
            Self::IncompleteStream => "响应流在完成前中断，结果未保存为成功。",
            Self::ProviderFailure { .. } => unreachable!("handled above"),
        };
        formatter.write_str(message)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    System,
    Direct,
}

struct ResponseOptions<'a> {
    instructions: Option<&'a str>,
    output_schema: Option<Value>,
    cancellation: Option<watch::Receiver<bool>>,
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) async fn list_chatgpt_models(
    app: AppHandle,
    manager: State<'_, Arc<super::siwc::ChatGptAuthManager>>,
    proxy_mode: ProxyMode,
) -> Result<Vec<AvailableModel>, String> {
    const MODEL_CATALOG_TIMEOUT: Duration = Duration::from_secs(60);
    let manager = manager.inner();
    let store = super::siwc::store_for_app(&app)?;
    let mut access_token = manager.access_token(&store).await?;
    let result = list_models(&access_token, MODEL_CATALOG_TIMEOUT, proxy_mode).await;
    access_token.zeroize();
    if matches!(&result, Err(InferenceError::ExpiredOrRevoked)) {
        manager.clear_confirmed_invalid_tokens(&store).await?;
        let _ = app.emit(
            "auth://status-changed",
            super::siwc::ChatGptConnectionStatus {
                connected: false,
                plan_usage_authorized: false,
            },
        );
    }
    let models = result.map_err(|error| error.to_string())?;
    let _ = app.emit("auth://model-catalog-updated", &models);
    Ok(models)
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) async fn test_chatgpt_inference(
    app: AppHandle,
    manager: State<'_, Arc<super::siwc::ChatGptAuthManager>>,
    model: String,
    proxy_mode: ProxyMode,
) -> Result<InferenceResult, String> {
    const INFERENCE_TIMEOUT: Duration = Duration::from_secs(90);
    let manager = manager.inner();
    let store = super::siwc::store_for_app(&app)?;
    let mut access_token = manager.access_token(&store).await?;
    let result = test_inference(&access_token, &model, INFERENCE_TIMEOUT, proxy_mode).await;
    access_token.zeroize();
    if matches!(&result, Err(InferenceError::ExpiredOrRevoked)) {
        manager.clear_confirmed_invalid_tokens(&store).await?;
        let _ = app.emit(
            "auth://status-changed",
            super::siwc::ChatGptConnectionStatus {
                connected: false,
                plan_usage_authorized: false,
            },
        );
    }
    let result = result.map_err(|error| error.to_string())?;
    let _ = app.emit(
        "auth://inference-event",
        serde_json::json!({"state":"completed","responseId":result.response_id}),
    );
    Ok(result)
}

pub fn build_http_client(
    timeout: Duration,
    proxy_mode: ProxyMode,
) -> Result<Client, InferenceError> {
    let route = if proxy_mode == ProxyMode::Direct {
        SystemProxyRoute::Direct
    } else {
        SystemProxyRoute::UseEnvironment
    };
    build_http_client_for_route(timeout, route)
}

#[derive(Clone)]
pub(super) enum SystemProxyRoute {
    UseEnvironment,
    Direct,
    Explicit {
        http_proxy: Option<String>,
        https_proxy: Option<String>,
        bypass: Vec<String>,
    },
}

async fn build_request_http_client(
    timeout: Duration,
    proxy_mode: ProxyMode,
    proxy_lookup_url: &str,
) -> Result<Client, InferenceError> {
    let route = match proxy_mode {
        ProxyMode::Direct => SystemProxyRoute::Direct,
        ProxyMode::System => {
            #[cfg(windows)]
            {
                let proxy_lookup_url = proxy_lookup_url.to_owned();
                tokio::task::spawn_blocking(move || {
                    crate::auth::system_proxy::resolve_current_user(&proxy_lookup_url)
                })
                .await
                .map_err(|_| InferenceError::ProxyOrNetwork)?
                .map_err(|_| InferenceError::ProxyOrNetwork)?
            }
            #[cfg(not(windows))]
            {
                SystemProxyRoute::UseEnvironment
            }
        }
    };

    build_http_client_for_route(timeout, route)
}

fn build_http_client_for_route(
    timeout: Duration,
    route: SystemProxyRoute,
) -> Result<Client, InferenceError> {
    let mut builder = Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(15));
    match route {
        SystemProxyRoute::UseEnvironment => {}
        SystemProxyRoute::Direct => builder = builder.no_proxy(),
        SystemProxyRoute::Explicit {
            http_proxy,
            https_proxy,
            bypass,
        } => {
            builder = builder.no_proxy();
            if http_proxy.is_some() || https_proxy.is_some() {
                builder = builder.proxy(reqwest::Proxy::custom(move |url| {
                    let host = url.host_str()?;
                    if windows_proxy_bypasses_host(host, &bypass) {
                        return None;
                    }
                    match url.scheme() {
                        "http" => http_proxy.clone(),
                        "https" => https_proxy.clone(),
                        _ => None,
                    }
                }));
            }
        }
    }
    builder.build().map_err(|_| InferenceError::ProxyOrNetwork)
}

fn windows_proxy_bypasses_host(host: &str, rules: &[String]) -> bool {
    let host = host.to_ascii_lowercase();
    rules.iter().any(|rule| {
        let rule = rule.trim().to_ascii_lowercase();
        if rule == "<local>" {
            return !host.contains('.');
        }
        windows_proxy_pattern_matches(&rule, &host)
    })
}

fn windows_proxy_pattern_matches(pattern: &str, host: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return host == suffix || host.ends_with(&format!(".{suffix}"));
    }
    if let Some(suffix) = pattern.strip_prefix('.') {
        return host == suffix || host.ends_with(&format!(".{suffix}"));
    }
    if !pattern.contains('*') {
        return host == pattern;
    }

    let mut remainder = host;
    for (index, part) in pattern.split('*').enumerate() {
        if part.is_empty() {
            continue;
        }
        if index == 0 {
            let Some(rest) = remainder.strip_prefix(part) else {
                return false;
            };
            remainder = rest;
        } else if let Some(position) = remainder.find(part) {
            remainder = &remainder[position + part.len()..];
        } else {
            return false;
        }
    }
    pattern.ends_with('*') || remainder.is_empty()
}

pub async fn list_models(
    access_token: &str,
    timeout: Duration,
    proxy_mode: ProxyMode,
) -> Result<Vec<AvailableModel>, InferenceError> {
    let proxy_lookup_url = format!("{OFFICIAL_API_BASE}/models");
    let client = build_request_http_client(timeout, proxy_mode, &proxy_lookup_url).await?;
    fetch_models_with_client(&client, OFFICIAL_API_BASE, access_token).await
}

pub async fn test_inference(
    access_token: &str,
    model: &str,
    timeout: Duration,
    proxy_mode: ProxyMode,
) -> Result<InferenceResult, InferenceError> {
    let proxy_lookup_url = format!("{OFFICIAL_API_BASE}/responses");
    let client = build_request_http_client(timeout, proxy_mode, &proxy_lookup_url).await?;
    send_response_with_client(
        &client,
        OFFICIAL_API_BASE,
        access_token,
        model,
        serde_json::json!([{"role":"user","content":"Reply with exactly: ARCANUM_AUTH_OK"}]),
    )
    .await
}

pub(crate) async fn reading_inference(
    access_token: &str,
    model: &str,
    instructions: &str,
    input: Value,
    timeout: Duration,
    proxy_mode: ProxyMode,
    cancellation: watch::Receiver<bool>,
) -> Result<InferenceResult, InferenceError> {
    reading_inference_with_schema(ReadingInferenceRequest {
        access_token,
        model,
        instructions,
        input,
        timeout,
        proxy_mode,
        cancellation,
        output_schema: crate::reading::contract::json_schema(),
    })
    .await
}

pub(crate) struct FollowupInferenceRequest<'a> {
    pub access_token: &'a str,
    pub model: &'a str,
    pub instructions: &'a str,
    pub input: Value,
    pub session_id: &'a str,
    pub reply_to_turn_id: &'a str,
    pub timeout: Duration,
    pub proxy_mode: ProxyMode,
    pub cancellation: watch::Receiver<bool>,
}

pub(crate) async fn followup_inference(
    request: FollowupInferenceRequest<'_>,
) -> Result<InferenceResult, InferenceError> {
    reading_inference_with_schema(ReadingInferenceRequest {
        access_token: request.access_token,
        model: request.model,
        instructions: request.instructions,
        input: request.input,
        timeout: request.timeout,
        proxy_mode: request.proxy_mode,
        cancellation: request.cancellation,
        output_schema: crate::reading::followup::json_schema_for_turn(
            request.session_id,
            request.reply_to_turn_id,
        ),
    })
    .await
}

struct ReadingInferenceRequest<'a> {
    access_token: &'a str,
    model: &'a str,
    instructions: &'a str,
    input: Value,
    timeout: Duration,
    proxy_mode: ProxyMode,
    cancellation: watch::Receiver<bool>,
    output_schema: Value,
}

async fn reading_inference_with_schema(
    request: ReadingInferenceRequest<'_>,
) -> Result<InferenceResult, InferenceError> {
    let proxy_lookup_url = format!("{OFFICIAL_API_BASE}/responses");
    let client =
        build_request_http_client(request.timeout, request.proxy_mode, &proxy_lookup_url).await?;
    send_response_with_options(
        &client,
        OFFICIAL_API_BASE,
        request.access_token,
        request.model,
        request.input,
        ResponseOptions {
            instructions: Some(request.instructions),
            output_schema: Some(request.output_schema),
            cancellation: Some(request.cancellation),
        },
    )
    .await
}

async fn fetch_models_with_client(
    client: &Client,
    api_base: &str,
    access_token: &str,
) -> Result<Vec<AvailableModel>, InferenceError> {
    let url = format!("{}/models", api_base.trim_end_matches('/'));
    let response = client
        .get(url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(classify_transport_error)?;
    if !response.status().is_success() {
        return Err(classify_http_failure(response).await);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_CATALOG_BYTES as u64)
    {
        return Err(InferenceError::InvalidResponse);
    }
    let body = read_limited_body(response, MAX_CATALOG_BYTES).await?;
    parse_model_catalog(&body)
}

fn parse_model_catalog(body: &[u8]) -> Result<Vec<AvailableModel>, InferenceError> {
    if body.len() > MAX_CATALOG_BYTES {
        return Err(InferenceError::InvalidResponse);
    }
    let document: ModelCatalog =
        serde_json::from_slice(body).map_err(|_| InferenceError::InvalidResponse)?;
    if document.models.len() > 2_000 {
        return Err(InferenceError::InvalidResponse);
    }
    let mut seen = std::collections::HashSet::new();
    let mut models = Vec::new();
    for item in document.models {
        let slug = item.slug.trim();
        let display_name = item.display_name.trim();
        if item.visibility != "list"
            || slug.is_empty()
            || slug.len() > 128
            || display_name.is_empty()
            || display_name.len() > 256
            || !slug
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            || !seen.insert(slug.to_string())
        {
            continue;
        }
        models.push(AvailableModel {
            slug: slug.to_string(),
            display_name: display_name.to_string(),
        });
    }
    Ok(models)
}

async fn send_response_with_client(
    client: &Client,
    api_base: &str,
    access_token: &str,
    model: &str,
    input: Value,
) -> Result<InferenceResult, InferenceError> {
    send_response_with_options(
        client,
        api_base,
        access_token,
        model,
        input,
        ResponseOptions {
            instructions: None,
            output_schema: None,
            cancellation: None,
        },
    )
    .await
}

async fn send_response_with_options(
    client: &Client,
    api_base: &str,
    access_token: &str,
    model: &str,
    input: Value,
    options: ResponseOptions<'_>,
) -> Result<InferenceResult, InferenceError> {
    let ResponseOptions {
        instructions,
        output_schema,
        mut cancellation,
    } = options;
    if model.is_empty()
        || model.len() > 128
        || !model
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(InferenceError::ModelUnavailable);
    }
    let url = format!("{}/responses", api_base.trim_end_matches('/'));
    let mut body = serde_json::json!({
        "model": model,
        "input": input,
        "store": false,
        "stream": true
    });
    if let Some(instructions) = instructions {
        body["instructions"] = Value::String(instructions.to_owned());
    }
    if let Some(schema) = output_schema {
        body["text"] = serde_json::json!({
            "format": {
                "type": "json_schema",
                "name": "arcanum_reading_v1",
                "strict": true,
                "schema": schema
            }
        });
    }
    let request = client
        .post(url)
        .bearer_auth(access_token)
        .header(reqwest::header::ACCEPT, "text/event-stream")
        .json(&body)
        .send();
    let response = if let Some(cancellation) = cancellation.as_mut() {
        tokio::select! {
            biased;
            _ = wait_for_cancellation(cancellation) => return Err(InferenceError::Cancelled),
            response = request => response.map_err(classify_transport_error)?,
        }
    } else {
        request.await.map_err(classify_transport_error)?
    };
    if !response.status().is_success() {
        return Err(classify_http_failure(response).await);
    }
    let mut response = response;
    let mut decoder = SseDecoder::default();
    loop {
        let chunk = if let Some(cancellation) = cancellation.as_mut() {
            tokio::select! {
                biased;
                _ = wait_for_cancellation(cancellation) => return Err(InferenceError::Cancelled),
                chunk = response.chunk() => chunk.map_err(classify_transport_error)?,
            }
        } else {
            response.chunk().await.map_err(classify_transport_error)?
        };
        let Some(chunk) = chunk else { break };
        decoder.push_chunk(&chunk)?;
    }
    decoder.finish()
}

async fn wait_for_cancellation(cancellation: &mut watch::Receiver<bool>) {
    if *cancellation.borrow() {
        return;
    }
    let _ = cancellation.changed().await;
}

async fn read_limited_body(
    mut response: Response,
    maximum: usize,
) -> Result<Vec<u8>, InferenceError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(classify_transport_error)? {
        if body.len().saturating_add(chunk.len()) > maximum {
            return Err(InferenceError::InvalidResponse);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn classify_http_failure(response: Response) -> InferenceError {
    let status = response.status();
    let body = read_limited_body(response, 64 * 1024)
        .await
        .unwrap_or_default();
    let code = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/code")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    match status {
        StatusCode::UNAUTHORIZED => InferenceError::ExpiredOrRevoked,
        StatusCode::FORBIDDEN => InferenceError::Unauthenticated,
        StatusCode::TOO_MANY_REQUESTS if code.as_deref().is_some_and(is_plan_limit_code) => {
            InferenceError::PlanLimit
        }
        StatusCode::TOO_MANY_REQUESTS => InferenceError::PlanLimit,
        StatusCode::NOT_FOUND => InferenceError::ModelUnavailable,
        _ if code.as_deref().is_some_and(is_plan_limit_code) => InferenceError::PlanLimit,
        _ if code.as_deref().is_some_and(is_model_unavailable_code) => {
            InferenceError::ModelUnavailable
        }
        _ => InferenceError::Server,
    }
}

fn is_plan_limit_code(code: &str) -> bool {
    matches!(
        code,
        "subscription_sharing_usage_limit_exceeded" | "subscription_sharing_usage_unavailable"
    )
}

fn is_model_unavailable_code(code: &str) -> bool {
    matches!(code, "model_not_found" | "unsupported_model")
}

fn is_rate_limit_code(code: &str) -> bool {
    matches!(code, "rate_limit_exceeded" | "slow_down")
}

fn is_server_failure_code(code: &str) -> bool {
    matches!(
        code,
        "server_error" | "server_overloaded" | "internal_server_error" | "service_unavailable"
    )
}

fn provider_failure_message(code: Option<&str>) -> &'static str {
    match code {
        Some(code) if is_plan_limit_code(code) => {
            "账号计划用量当前受限或不可用，请检查ChatGPT账号用量后重试。"
        }
        Some(code) if is_rate_limit_code(code) => "请求频率受到限制，请稍后再试。",
        Some(code) if is_model_unavailable_code(code) => {
            "所选模型当前不可用，请在设置中刷新并重新选择模型。"
        }
        Some(code) if is_server_failure_code(code) => "OpenAI服务暂时无法完成请求，请稍后重试。",
        _ => "OpenAI未能完成本次响应，可稍后重试；错误码和响应编号可用于排查。",
    }
}

fn validated_provider_error_code(code: &str) -> Option<String> {
    (!code.is_empty()
        && code.len() <= 96
        && code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')))
    .then(|| code.to_owned())
}

fn validated_response_id(response_id: &str) -> Option<String> {
    let suffix = response_id.strip_prefix("resp_")?;
    (!suffix.is_empty()
        && response_id.len() <= 128
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
    .then(|| response_id.to_owned())
}

fn provider_failure_from_event(
    event: &Value,
    fallback_response_id: Option<&str>,
) -> InferenceError {
    let code = event
        .pointer("/response/error/code")
        .or_else(|| event.pointer("/error/code"))
        .or_else(|| event.get("code"))
        .and_then(Value::as_str)
        .and_then(validated_provider_error_code);
    let response_id = event
        .pointer("/response/id")
        .or_else(|| event.get("response_id"))
        .and_then(Value::as_str)
        .and_then(validated_response_id)
        .or_else(|| fallback_response_id.and_then(validated_response_id));

    InferenceError::ProviderFailure { code, response_id }
}

fn classify_transport_error(error: reqwest::Error) -> InferenceError {
    if error.is_timeout() {
        InferenceError::Timeout
    } else {
        InferenceError::ProxyOrNetwork
    }
}

#[derive(Deserialize)]
struct ModelCatalog {
    models: Vec<ModelCatalogItem>,
}

#[derive(Deserialize)]
struct ModelCatalogItem {
    slug: String,
    display_name: String,
    visibility: String,
}

#[derive(Default)]
struct SseDecoder {
    buffered: Vec<u8>,
    output_text: String,
    response_id: Option<String>,
    completed: bool,
    total_bytes: usize,
}

impl SseDecoder {
    fn push_chunk(&mut self, chunk: &[u8]) -> Result<(), InferenceError> {
        self.total_bytes = self.total_bytes.saturating_add(chunk.len());
        if self.total_bytes > MAX_STREAM_BYTES {
            return Err(InferenceError::InvalidResponse);
        }
        self.buffered.extend_from_slice(chunk);
        while let Some((start, length)) = find_frame_separator(&self.buffered) {
            let frame = self.buffered[..start].to_vec();
            self.buffered.drain(..start + length);
            self.process_frame(&frame)?;
        }
        Ok(())
    }

    fn process_frame(&mut self, frame: &[u8]) -> Result<(), InferenceError> {
        if frame.is_empty() || self.completed {
            return Ok(());
        }
        let frame = std::str::from_utf8(frame).map_err(|_| InferenceError::InvalidResponse)?;
        let data = frame
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() || data == "[DONE]" {
            return Ok(());
        }
        let event: Value =
            serde_json::from_str(&data).map_err(|_| InferenceError::InvalidResponse)?;
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .ok_or(InferenceError::InvalidResponse)?;
        if let Some(response_id) = event
            .pointer("/response/id")
            .or_else(|| event.get("response_id"))
            .and_then(Value::as_str)
            .and_then(validated_response_id)
        {
            self.response_id = Some(response_id);
        }
        match kind {
            "response.output_text.delta" => {
                let delta = event
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or(InferenceError::InvalidResponse)?;
                if self.output_text.len().saturating_add(delta.len()) > MAX_OUTPUT_BYTES {
                    return Err(InferenceError::InvalidResponse);
                }
                self.output_text.push_str(delta);
            }
            "response.output_text.done" if self.output_text.is_empty() => {
                if let Some(text) = event.get("text").and_then(Value::as_str) {
                    if text.len() > MAX_OUTPUT_BYTES {
                        return Err(InferenceError::InvalidResponse);
                    }
                    self.output_text.push_str(text);
                }
            }
            "response.completed" => {
                let response = event
                    .get("response")
                    .ok_or(InferenceError::InvalidResponse)?;
                if let Some(response_id) = response
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(validated_response_id)
                {
                    self.response_id = Some(response_id);
                }
                if self.output_text.is_empty() {
                    self.output_text = extract_completed_text(response)?;
                }
                self.completed = true;
            }
            "response.failed" | "error" => {
                return Err(provider_failure_from_event(
                    &event,
                    self.response_id.as_deref(),
                ));
            }
            "response.incomplete" => return Err(InferenceError::IncompleteStream),
            _ => {}
        }
        Ok(())
    }

    fn finish(mut self) -> Result<InferenceResult, InferenceError> {
        if !self.buffered.is_empty() {
            let frame = std::mem::take(&mut self.buffered);
            self.process_frame(&frame)?;
        }
        if !self.completed {
            return Err(InferenceError::IncompleteStream);
        }
        Ok(InferenceResult {
            output_text: self.output_text,
            response_id: self.response_id,
        })
    }
}

fn find_frame_separator(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, 2));
    let crlf = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4));
    match (lf, crlf) {
        (Some(left), Some(right)) if left.0 <= right.0 => Some(left),
        (Some(_), Some(right)) => Some(right),
        (Some(found), None) | (None, Some(found)) => Some(found),
        (None, None) => None,
    }
}

fn extract_completed_text(response: &Value) -> Result<String, InferenceError> {
    let mut output = String::new();
    let Some(items) = response.get("output").and_then(Value::as_array) else {
        return Ok(output);
    };
    for item in items {
        let Some(contents) = item.get("content").and_then(Value::as_array) else {
            continue;
        };
        for content in contents {
            if content.get("type").and_then(Value::as_str) == Some("output_text") {
                if let Some(text) = content.get("text").and_then(Value::as_str) {
                    if output.len().saturating_add(text.len()) > MAX_OUTPUT_BYTES {
                        return Err(InferenceError::InvalidResponse);
                    }
                    output.push_str(text);
                }
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{
        build_http_client, fetch_models_with_client, parse_model_catalog,
        send_response_with_client, send_response_with_options, InferenceError, ProxyMode,
        ResponseOptions, SseDecoder,
    };
    use serde_json::json;
    use std::time::Duration;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[test]
    fn model_catalog_includes_only_valid_visible_unique_models() {
        let models = parse_model_catalog(
            br#"{"models":[{"slug":"gpt-fast","display_name":"GPT Fast","visibility":"list"},{"slug":"hidden","display_name":"Hidden","visibility":"hide"},{"slug":"gpt-fast","display_name":"Duplicate","visibility":"list"},{"slug":"","display_name":"Missing slug","visibility":"list"}]}"#,
        )
        .unwrap();

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].slug, "gpt-fast");
        assert_eq!(models[0].display_name, "GPT Fast");
    }

    #[test]
    fn model_catalog_does_not_invent_models_for_empty_or_invalid_documents() {
        assert!(parse_model_catalog(br#"{"models":[]}"#).unwrap().is_empty());
        assert!(parse_model_catalog(br#"{"data":[{"id":"not-a-model-shape"}]}"#).is_err());
    }

    #[test]
    fn responses_sse_yields_text_only_after_completed_event() {
        let mut decoder = SseDecoder::default();
        decoder
            .push_chunk(
                b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"A clear answer\"}\n\n",
            )
            .unwrap();
        decoder
            .push_chunk(
                b"data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_test\"}}\n\n",
            )
            .unwrap();
        let result = decoder.finish().unwrap();

        assert_eq!(result.output_text, "A clear answer");
        assert_eq!(result.response_id.as_deref(), Some("resp_test"));
    }

    #[test]
    fn responses_sse_rejects_failed_and_incomplete_streams() {
        let mut failed = SseDecoder::default();
        assert!(matches!(
            failed.push_chunk(b"data: {\"type\":\"response.failed\",\"response\":{}}\n\n"),
            Err(InferenceError::ProviderFailure {
                code: None,
                response_id: None
            })
        ));

        let mut incomplete = SseDecoder::default();
        incomplete
            .push_chunk(
                b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
            )
            .unwrap();
        assert!(matches!(
            incomplete.finish(),
            Err(InferenceError::IncompleteStream)
        ));
    }

    #[test]
    fn responses_sse_keeps_split_utf8_sequences_intact() {
        let event = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"牌面\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n".as_bytes();
        let split = event
            .windows(1)
            .position(|window| window[0] >= 0x80)
            .unwrap()
            + 1;
        let mut decoder = SseDecoder::default();
        decoder.push_chunk(&event[..split]).unwrap();
        decoder.push_chunk(&event[split..]).unwrap();

        assert_eq!(decoder.finish().unwrap().output_text, "牌面");
    }

    #[test]
    fn responses_sse_rejects_oversized_streams_and_malformed_events() {
        let mut malformed = SseDecoder::default();
        assert!(matches!(
            malformed.push_chunk(b"data: {broken json}\n\n"),
            Err(InferenceError::InvalidResponse)
        ));

        let mut oversized = SseDecoder::default();
        assert!(oversized.push_chunk(&vec![b'x'; 20 * 1024 * 1024]).is_err());
    }

    #[tokio::test]
    async fn model_request_uses_official_catalog_shape_and_bearer_auth() {
        let (endpoint, captured) = mock_http_once(
            "200 OK",
            "application/json",
            br#"{"models":[{"slug":"gpt-live","display_name":"GPT Live","visibility":"list"}]}"#,
        )
        .await;
        let client = build_http_client(Duration::from_secs(2), ProxyMode::Direct).unwrap();

        let models =
            fetch_models_with_client(&client, &format!("{endpoint}/v1"), "synthetic-test-token")
                .await
                .unwrap();
        let request = captured.await.unwrap();

        assert_eq!(models[0].slug, "gpt-live");
        assert!(request.starts_with("GET /v1/models HTTP/1.1"));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer synthetic-test-token"));
    }

    #[tokio::test]
    async fn responses_request_sets_only_supported_storage_and_stream_flags() {
        let (endpoint, captured) = mock_http_once(
            "200 OK",
            "text/event-stream",
            b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"ready\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_test\"}}\n\n",
        )
        .await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let result = send_response_with_client(
            &client,
            &format!("{endpoint}/v1"),
            "synthetic-test-token",
            "gpt-live",
            json!([{"role":"user","content":"Say ready"}]),
        )
        .await
        .unwrap();
        let request = captured.await.unwrap();
        let body_start = request.find("\r\n\r\n").unwrap() + 4;
        let body: serde_json::Value = serde_json::from_str(&request[body_start..]).unwrap();

        assert_eq!(result.output_text, "ready");
        assert_eq!(result.response_id.as_deref(), Some("resp_test"));
        assert!(request.starts_with("POST /v1/responses HTTP/1.1"));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer synthetic-test-token"));
        assert_eq!(body["model"], "gpt-live");
        assert_eq!(body["input"][0]["content"], "Say ready");
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        for forbidden in ["max_output_tokens", "temperature", "previous_response_id"] {
            assert!(body.get(forbidden).is_none());
        }
    }

    #[tokio::test]
    async fn reading_responses_send_instructions_and_strict_schema_without_system_role() {
        let (endpoint, captured) = mock_http_once(
            "200 OK",
            "text/event-stream",
            b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"{}\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n",
        )
        .await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let input = json!([
            {"role":"developer","content":"trusted frozen spread"},
            {"role":"user","content":"untrusted question"}
        ]);
        let schema = crate::reading::contract::json_schema();
        send_response_with_options(
            &client,
            &format!("{endpoint}/v1"),
            "synthetic-test-token",
            "gpt-live",
            input,
            super::ResponseOptions {
                instructions: Some("fixed trusted instructions"),
                output_schema: Some(schema),
                cancellation: None,
            },
        )
        .await
        .unwrap();
        let request = captured.await.unwrap();
        let body_start = request.find("\r\n\r\n").unwrap() + 4;
        let body: serde_json::Value = serde_json::from_str(&request[body_start..]).unwrap();

        assert_eq!(body["instructions"], "fixed trusted instructions");
        assert_eq!(body["input"][0]["role"], "developer");
        assert_eq!(body["input"][1]["role"], "user");
        assert_eq!(body["text"]["format"]["type"], "json_schema");
        assert_eq!(body["text"]["format"]["strict"], true);
        assert_eq!(
            body["text"]["format"]["schema"]["properties"]["sessionId"]["type"],
            "string"
        );
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert!(!body["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "system"));
    }

    #[tokio::test]
    async fn followup_responses_use_turn_bound_citation_schema() {
        let (endpoint, captured) = mock_http_once(
            "200 OK",
            "text/event-stream",
            b"data: {\"type\":\"response.completed\",\"response\":{}}\n\n",
        )
        .await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        send_response_with_options(
            &client,
            &format!("{endpoint}/v1"),
            "synthetic-test-token",
            "gpt-live",
            json!([{"role":"user","content":"follow-up"}]),
            ResponseOptions {
                instructions: Some("preserve prior stance"),
                output_schema: Some(crate::reading::followup::json_schema_for_turn(
                    "session-1",
                    "turn-1",
                )),
                cancellation: None,
            },
        )
        .await
        .unwrap();
        let request = captured.await.unwrap();
        let body_start = request.find("\r\n\r\n").unwrap() + 4;
        let body: serde_json::Value = serde_json::from_str(&request[body_start..]).unwrap();

        let schema = &body["text"]["format"]["schema"];
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("replyToTurnId")));
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("stanceUnchanged")));
        assert!(schema["properties"]["citedCards"]["items"]["properties"]
            .get("cardId")
            .is_some());
        assert_eq!(schema["properties"]["schemaVersion"]["enum"][0], 1);
        assert_eq!(schema["properties"]["sessionId"]["enum"][0], "session-1");
        assert_eq!(schema["properties"]["replyToTurnId"]["enum"][0], "turn-1");
        assert_eq!(schema["properties"]["stanceUnchanged"]["enum"][0], true);
    }

    #[tokio::test]
    async fn reading_request_cancellation_prevents_network_send() {
        let (_sender, receiver) = tokio::sync::watch::channel(true);
        let result = super::reading_inference(
            "synthetic-test-token",
            "gpt-live",
            "fixed instructions",
            json!([{"role":"user","content":"question"}]),
            Duration::from_secs(2),
            ProxyMode::Direct,
            receiver,
        )
        .await;

        assert!(matches!(result, Err(InferenceError::Cancelled)));
    }

    #[tokio::test]
    async fn reading_request_cancellation_stops_an_open_sse_stream() {
        let (endpoint, first_event, server) = mock_http_partial_stream().await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let (cancel_sender, cancel_receiver) = tokio::sync::watch::channel(false);
        let request = tokio::spawn(async move {
            send_response_with_options(
                &client,
                &format!("{endpoint}/v1"),
                "synthetic-test-token",
                "gpt-live",
                json!([{"role":"user","content":"question"}]),
                ResponseOptions {
                    instructions: Some("fixed instructions"),
                    output_schema: Some(crate::reading::contract::json_schema()),
                    cancellation: Some(cancel_receiver),
                },
            )
            .await
        });
        first_event.await.unwrap();
        cancel_sender.send(true).unwrap();

        assert!(matches!(
            request.await.unwrap(),
            Err(InferenceError::Cancelled)
        ));
        server.abort();
    }

    #[tokio::test]
    async fn responses_classifies_expired_auth_and_chatgpt_usage_limits() {
        for (status, body, expected) in [
            (
                "401 Unauthorized",
                br#"{"error":{"code":"invalid_token"}}"#.as_slice(),
                "expired",
            ),
            (
                "403 Forbidden",
                br#"{"error":{"code":"insufficient_scope"}}"#.as_slice(),
                "unauthorized",
            ),
            (
                "429 Too Many Requests",
                br#"{"error":{"code":"subscription_sharing_usage_limit_exceeded"}}"#.as_slice(),
                "limit",
            ),
        ] {
            let (endpoint, _) = mock_http_once(status, "application/json", body).await;
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let error = send_response_with_client(
                &client,
                &endpoint,
                "synthetic-test-token",
                "gpt-live",
                json!([{"role":"user","content":"probe"}]),
            )
            .await
            .unwrap_err();

            match expected {
                "expired" => assert!(matches!(error, InferenceError::ExpiredOrRevoked)),
                "unauthorized" => assert!(matches!(error, InferenceError::Unauthenticated)),
                "limit" => assert!(matches!(error, InferenceError::PlanLimit)),
                _ => unreachable!(),
            }
        }
    }

    #[tokio::test]
    async fn model_catalog_timeout_is_classified_and_direct_route_is_available_for_retry() {
        let (endpoint, server) = mock_http_hang().await;
        let client = build_http_client(Duration::from_millis(50), ProxyMode::Direct).unwrap();
        let error =
            fetch_models_with_client(&client, &format!("{endpoint}/v1"), "synthetic-test-token")
                .await
                .unwrap_err();
        assert!(matches!(error, InferenceError::Timeout));
        server.abort();

        let (endpoint, captured) = mock_http_once(
            "200 OK",
            "application/json",
            br#"{"models":[{"slug":"gpt-retry","display_name":"GPT Retry","visibility":"list"}]}"#,
        )
        .await;
        let direct_client = build_http_client(Duration::from_secs(2), ProxyMode::Direct).unwrap();
        let models = fetch_models_with_client(
            &direct_client,
            &format!("{endpoint}/v1"),
            "synthetic-test-token",
        )
        .await
        .unwrap();
        let _request = captured.await.unwrap();
        assert_eq!(models[0].slug, "gpt-retry");
    }

    async fn mock_http_once(
        status: &'static str,
        content_type: &'static str,
        body: &'static [u8],
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let body = body.to_vec();
        let captured = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            let header_end = loop {
                let read = socket.read(&mut chunk).await.unwrap();
                assert_ne!(read, 0, "client closed before sending HTTP headers");
                request.extend_from_slice(&chunk[..read]);
                if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            while request.len().saturating_sub(header_end) < content_length {
                let read = socket.read(&mut chunk).await.unwrap();
                assert_ne!(read, 0, "client closed before sending request body");
                request.extend_from_slice(&chunk[..read]);
            }
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.write_all(&body).await.unwrap();
            String::from_utf8(request).unwrap()
        });
        (format!("http://{address}"), captured)
    }

    async fn mock_http_hang() -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut chunk = [0; 4096];
            let _ = socket.read(&mut chunk).await;
            tokio::time::sleep(Duration::from_secs(1)).await;
        });
        (format!("http://{address}"), server)
    }

    async fn mock_http_partial_stream() -> (
        String,
        tokio::sync::oneshot::Receiver<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (ready_sender, ready_receiver) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = socket.read(&mut chunk).await.unwrap();
                if read == 0 {
                    return;
                }
                request.extend_from_slice(&chunk[..read]);
            }
            let event =
                b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n";
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: keep-alive\r\n\r\n")
                .await
                .unwrap();
            socket
                .write_all(format!("{:x}\r\n", event.len()).as_bytes())
                .await
                .unwrap();
            socket.write_all(event).await.unwrap();
            socket.write_all(b"\r\n").await.unwrap();
            let _ = ready_sender.send(());
            std::future::pending::<()>().await;
        });
        (format!("http://{address}"), ready_receiver, server)
    }
}
