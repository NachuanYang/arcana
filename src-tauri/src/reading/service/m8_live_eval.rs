use super::{generate_followup_response, generate_reading_response, FollowupFailure};
use crate::auth::credential_store::CredentialStore;
use crate::auth::inference::{list_models, AvailableModel, InferenceError, ProxyMode};
use crate::auth::siwc::ChatGptAuthManager;
use crate::persistence::SessionRepository;
use crate::tarot::deck::{TarotCard, TarotDeck, TAROT_DATASET_VERSION};
use crate::tarot::draw_engine::{validate_question, DrawnCard, RevealSnapshot};
use chrono::Utc;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use uuid::Uuid;
use zeroize::Zeroizing;

const APPROVAL_ENV: &str = "ARCANUM_M8_LIVE_APPROVED";
const APPROVAL_VALUE: &str = "110_SYNTHETIC_GPT_6_LUNA";
const MODEL_SLUG: &str = "gpt-6-luna";
const MODEL_EFFORT: &str = "provider default; the product request does not set reasoning effort";
const PROXY_MODE: ProxyMode = ProxyMode::System;
const EXPECTED_READING_CASES: usize = 60;
const EXPECTED_FOLLOWUP_CASES: usize = 30;
const EXPECTED_SENSITIVE_CASES: usize = 20;

static PROVIDER_REQUEST_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);

#[test]
fn profile_store_resolves_to_the_existing_arcanum_token_record() {
    let store = app_credential_store().unwrap();
    assert!(store.tokens_path().is_file());
}

#[test]
fn model_selection_requires_the_exact_gpt_6_luna_slug() {
    let catalog = [
        AvailableModel {
            slug: "gpt-6-luna-preview".into(),
            display_name: "GPT-6-Luna Preview".into(),
        },
        AvailableModel {
            slug: "gpt-6-sol".into(),
            display_name: "GPT-6-Sol".into(),
        },
        AvailableModel {
            slug: MODEL_SLUG.into(),
            display_name: "GPT-6-Luna".into(),
        },
    ];

    assert_eq!(exact_luna_model(&catalog).unwrap().slug, MODEL_SLUG);
    assert!(exact_luna_model(&catalog[..2]).is_err());
}

#[test]
fn all_frozen_cases_match_the_bundled_runtime_deck_before_any_request() {
    let workspace = workspace_root();
    let readings: Vec<ReadingCase> =
        load_jsonl(&workspace.join("tools/eval/corpus/m8_readings_60.jsonl")).unwrap();
    let followups: Vec<FollowupCase> =
        load_jsonl(&workspace.join("tools/eval/corpus/m8_followups_30.jsonl")).unwrap();
    let sensitive: Vec<SensitiveCase> =
        load_jsonl(&workspace.join("tools/eval/corpus/m8_sensitive_20.jsonl")).unwrap();
    let deck = TarotDeck::load_bundled().unwrap();
    let cards_by_id: HashMap<String, TarotCard> = deck
        .cards
        .into_iter()
        .map(|card| (card.id.clone(), card))
        .collect();

    validate_evaluation_inputs(&readings, &followups, &sensitive, &cards_by_id).unwrap();
}

#[test]
#[ignore = "read-only account catalog preflight; requires the approved M8 authorization marker"]
fn preflight_exact_luna_catalog_without_inference() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create M8 preflight runtime")
        .block_on(async {
            require_batch_approval()?;
            let store = app_credential_store()?;
            let auth = ChatGptAuthManager::default();
            let model = require_luna_catalog_model(&auth, &store).await?;
            println!(
                "M8 catalog preflight PASS: slug={} displayName={}; no inference request sent",
                model.slug, model.display_name
            );
            Ok::<(), String>(())
        })
        .unwrap_or_else(|reason| panic!("M8 catalog preflight failed: {reason}"));
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct EvalCard {
    card_id: String,
    slot: String,
    orientation: String,
    name_zh: String,
    name_en: String,
    keywords: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadingCase {
    id: String,
    question: String,
    deck_version: String,
    spread: Vec<EvalCard>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FollowupCase {
    id: String,
    question: String,
    reading_case_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SensitiveCase {
    id: String,
    question: String,
    reading_case_id: String,
    risk_category: String,
    expected_handling: String,
}

struct CaseOutcome {
    session_id: String,
    validated: bool,
    request_attempts: usize,
    stop_code: Option<&'static str>,
}

struct CaseServices<'a> {
    cards_by_id: &'a HashMap<String, TarotCard>,
    auth: &'a ChatGptAuthManager,
    store: &'a CredentialStore,
    repository: &'a SessionRepository,
    dataset_version: &'a str,
    prompt_version: &'a str,
}

struct ReadingCaseRequest<'a> {
    case_type: &'a str,
    case_id: &'a str,
    question: &'a str,
    spread_source: &'a ReadingCase,
    sensitive_case: Option<&'a SensitiveCase>,
}

struct CaseRecordInput<'a> {
    case_type: &'a str,
    case_id: &'a str,
    question: &'a str,
    session_id: &'a str,
    request_id: &'a str,
    cards: &'a [DrawnCard],
    dataset_version: &'a str,
    prompt_version: &'a str,
}

pub(super) fn record_provider_request_attempt() {
    PROVIDER_REQUEST_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
}

#[test]
#[ignore = "consumes the explicitly authorized ChatGPT plan allowance"]
fn run_authorized_m8_luna_batch() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create M8 test runtime")
        .block_on(run_batch())
        .unwrap_or_else(|reason| panic!("M8 live evaluation stopped: {reason}"));
}

async fn run_batch() -> Result<(), String> {
    require_batch_approval()?;

    let workspace = workspace_root();
    let readings: Vec<ReadingCase> =
        load_jsonl(&workspace.join("tools/eval/corpus/m8_readings_60.jsonl"))?;
    let followups: Vec<FollowupCase> =
        load_jsonl(&workspace.join("tools/eval/corpus/m8_followups_30.jsonl"))?;
    let sensitive: Vec<SensitiveCase> =
        load_jsonl(&workspace.join("tools/eval/corpus/m8_sensitive_20.jsonl"))?;
    let readings_by_id: HashMap<String, ReadingCase> = readings
        .iter()
        .cloned()
        .map(|case| (case.id.clone(), case))
        .collect();
    let deck = TarotDeck::load_bundled().map_err(|_| "runtime tarot deck validation failed")?;
    let cards_by_id: HashMap<String, TarotCard> = deck
        .cards
        .into_iter()
        .map(|card| (card.id.clone(), card))
        .collect();
    validate_evaluation_inputs(&readings, &followups, &sensitive, &cards_by_id)?;
    let dataset_version = fingerprint_files(
        &workspace,
        &[
            "tools/eval/corpus/m8_readings_60.jsonl",
            "tools/eval/corpus/m8_followups_30.jsonl",
            "tools/eval/corpus/m8_sensitive_20.jsonl",
        ],
    )?;
    let prompt_version = fingerprint_files(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        &[
            "src/reading/prompt.rs",
            "src/reading/followup.rs",
            "src/reading/contract.rs",
            "src/auth/inference.rs",
            "src/reading/service.rs",
        ],
    )?;

    let store = app_credential_store()?;
    let auth = ChatGptAuthManager::default();
    let selected_model = require_luna_catalog_model(&auth, &store).await?;
    let model_display_name = selected_model.display_name.clone();

    let results_directory = workspace.join("tools/eval/results");
    fs::create_dir_all(&results_directory)
        .map_err(|_| "could not create the local M8 results directory")?;
    let run_id = Uuid::new_v4().simple().to_string();
    let started_at = Utc::now().to_rfc3339();
    let file_name = format!(
        "live-m8-gpt-6-luna-{}-{run_id}.jsonl",
        Utc::now().format("%Y%m%dT%H%M%SZ")
    );
    let output_path = results_directory.join(file_name);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_path)
        .map_err(|_| "could not create a unique M8 results file")?;
    let mut output = BufWriter::new(file);
    write_jsonl(
        &mut output,
        &json!({
            "recordType":"manifest",
            "runId":run_id,
            "startedAt":started_at,
            "authorization":"user-approved synthetic M8 batch",
            "selectedModel":MODEL_SLUG,
            "modelDisplayName":model_display_name,
            "modelEffort":MODEL_EFFORT,
            "proxyMode":"system",
            "deckVersion":TAROT_DATASET_VERSION,
            "datasetVersion":dataset_version,
            "promptVersion":prompt_version,
            "plannedCases":{
                "readings":EXPECTED_READING_CASES,
                "followups":EXPECTED_FOLLOWUP_CASES,
                "sensitive":EXPECTED_SENSITIVE_CASES,
                "total":EXPECTED_READING_CASES + EXPECTED_FOLLOWUP_CASES + EXPECTED_SENSITIVE_CASES
            },
            "excludedGroups":["m8_holdout_20.jsonl","m8_failures_12.jsonl"],
            "outputSemantics":"modelOutput contains the production-validated JSON shown by the app; invalid output is recorded by failureCode",
            "ttftMeasured":false
        }),
    )?;
    println!("M8 GPT-6-Luna result file: {}", output_path.display());

    let temporary_database = tempfile::tempdir()
        .map_err(|_| "could not create an isolated temporary SQLite directory")?;
    let repository =
        SessionRepository::open(temporary_database.path().join("synthetic-m8.sqlite3"))?;
    let services = CaseServices {
        cards_by_id: &cards_by_id,
        auth: &auth,
        store: &store,
        repository: &repository,
        dataset_version: &dataset_version,
        prompt_version: &prompt_version,
    };
    let mut sessions_by_reading: HashMap<String, String> = HashMap::new();
    let mut successful_readings = HashSet::new();
    let mut completed_cases = 0usize;
    let mut validated_cases = 0usize;
    let mut blocked_cases = 0usize;
    let mut request_attempts = 0usize;
    let mut stop_code = None;
    let total_cases = EXPECTED_READING_CASES + EXPECTED_FOLLOWUP_CASES + EXPECTED_SENSITIVE_CASES;

    for (index, case) in readings.iter().enumerate() {
        println!("M8 {}/{} reading {}", index + 1, total_cases, case.id);
        let outcome = run_reading_case(
            ReadingCaseRequest {
                case_type: "reading",
                case_id: &case.id,
                question: &case.question,
                spread_source: case,
                sensitive_case: None,
            },
            &services,
            &mut output,
        )
        .await?;
        completed_cases += 1;
        request_attempts += outcome.request_attempts;
        sessions_by_reading.insert(case.id.clone(), outcome.session_id);
        if outcome.validated {
            successful_readings.insert(case.id.clone());
            validated_cases += 1;
        }
        if let Some(code) = outcome.stop_code {
            stop_code = Some(code);
            break;
        }
    }

    if stop_code.is_none() {
        for (offset, case) in followups.iter().enumerate() {
            let case_number = EXPECTED_READING_CASES + offset + 1;
            println!("M8 {case_number}/{total_cases} followup {}", case.id);
            let Some(session_id) = sessions_by_reading.get(&case.reading_case_id) else {
                return Err("internal M8 session index is incomplete".into());
            };
            if !successful_readings.contains(&case.reading_case_id) {
                write_jsonl(
                    &mut output,
                    &json!({
                        "recordType":"case",
                        "caseType":"followup",
                        "caseId":case.id,
                        "readingCaseId":case.reading_case_id,
                        "sessionId":session_id,
                        "question":case.question,
                        "modelId":MODEL_SLUG,
                        "modelEffort":MODEL_EFFORT,
                        "datasetVersion":dataset_version,
                        "promptVersion":prompt_version,
                        "status":"BLOCKED_DEPENDENCY",
                        "failureCode":"source_reading_unavailable",
                        "requestAttempts":0,
                        "modelOutput":Value::Null
                    }),
                )?;
                completed_cases += 1;
                blocked_cases += 1;
                continue;
            }
            let (validated, attempts, fatal_code) = run_followup_case(
                case,
                session_id,
                &readings_by_id[&case.reading_case_id],
                &services,
                &mut output,
            )
            .await?;
            completed_cases += 1;
            request_attempts += attempts;
            if validated {
                validated_cases += 1;
            }
            if let Some(code) = fatal_code {
                stop_code = Some(code);
                break;
            }
        }
    }

    if stop_code.is_none() {
        for (offset, case) in sensitive.iter().enumerate() {
            let case_number = EXPECTED_READING_CASES + EXPECTED_FOLLOWUP_CASES + offset + 1;
            println!("M8 {case_number}/{total_cases} sensitive {}", case.id);
            let source_case = readings_by_id
                .get(&case.reading_case_id)
                .ok_or_else(|| "sensitive case references an unknown reading".to_string())?;
            let outcome = run_reading_case(
                ReadingCaseRequest {
                    case_type: "sensitive",
                    case_id: &case.id,
                    question: &case.question,
                    spread_source: source_case,
                    sensitive_case: Some(case),
                },
                &services,
                &mut output,
            )
            .await?;
            completed_cases += 1;
            request_attempts += outcome.request_attempts;
            if outcome.validated {
                validated_cases += 1;
            }
            if let Some(code) = outcome.stop_code {
                stop_code = Some(code);
                break;
            }
        }
    }

    write_jsonl(
        &mut output,
        &json!({
            "recordType":"summary",
            "finishedAt":Utc::now().to_rfc3339(),
            "allCasesProcessed":completed_cases == total_cases,
            "caseRowsWritten":completed_cases,
            "plannedCases":total_cases,
            "validatedCases":validated_cases,
            "blockedCases":blocked_cases,
            "providerRequestAttempts":request_attempts,
            "stopCode":stop_code,
            "aiQuality":"NOT_SCORED_PENDING_TWO_INDEPENDENT_BLIND_REVIEWERS"
        }),
    )?;
    println!(
        "M8 execution summary: cases={completed_cases}/{total_cases}, validated={validated_cases}, blocked={blocked_cases}, providerAttempts={request_attempts}, stop={:?}",
        stop_code
    );
    if completed_cases != total_cases {
        return Err(format!(
            "batch stopped after {completed_cases}/{total_cases} case rows; failure code: {}",
            stop_code.unwrap_or("batch_incomplete")
        ));
    }
    Ok(())
}

async fn run_reading_case(
    request: ReadingCaseRequest<'_>,
    services: &CaseServices<'_>,
    output: &mut BufWriter<File>,
) -> Result<CaseOutcome, String> {
    let ReadingCaseRequest {
        case_type,
        case_id,
        question,
        spread_source,
        sensitive_case,
    } = request;
    let reveal = frozen_reveal(question, spread_source, services.cards_by_id)?;
    services.repository.persist_reveal(&reveal)?;
    let session_id = reveal.draw_id.clone();
    let session = services
        .repository
        .get_session(&session_id)?
        .ok_or_else(|| "temporary M8 session was not persisted".to_string())?;
    let request_id = Uuid::new_v4().to_string();
    let turn = services
        .repository
        .begin_reading_turn(&session_id, &request_id, MODEL_SLUG)?;
    services.repository.transition_reading_turn(
        &session_id,
        &turn.id,
        r#"{"stage":"streaming"}"#,
        "STREAMING",
    )?;

    let mut record = case_common_record(CaseRecordInput {
        case_type,
        case_id,
        question,
        session_id: &session_id,
        request_id: &request_id,
        cards: &reveal.cards,
        dataset_version: services.dataset_version,
        prompt_version: services.prompt_version,
    });
    if let Some(sensitive) = sensitive_case {
        record["readingCaseId"] = json!(sensitive.reading_case_id);
        record["riskCategory"] = json!(sensitive.risk_category);
        record["expectedHandling"] = json!(sensitive.expected_handling);
    }

    let token = match services.auth.access_token(services.store).await {
        Ok(token) => Zeroizing::new(token),
        Err(_) => {
            let code = "authorization_unavailable";
            services.repository.transition_reading_turn(
                &session_id,
                &turn.id,
                &json!({"stage":"failed","failureCode":code}).to_string(),
                "FAILED_RETRYABLE",
            )?;
            record["status"] = json!("FAILED");
            record["failureCode"] = json!(code);
            record["requestAttempts"] = json!(0);
            record["latencyMs"] = json!(0);
            record["modelOutput"] = Value::Null;
            write_jsonl(output, &record)?;
            return Ok(CaseOutcome {
                session_id,
                validated: false,
                request_attempts: 0,
                stop_code: Some(code),
            });
        }
    };

    PROVIDER_REQUEST_ATTEMPTS.store(0, Ordering::Relaxed);
    let started = Instant::now();
    let (_cancel_sender, cancellation) = watch::channel(false);
    let result =
        generate_reading_response(&session, MODEL_SLUG, PROXY_MODE, &token, cancellation).await;
    let latency_ms = started.elapsed().as_millis();
    let attempts = PROVIDER_REQUEST_ATTEMPTS.swap(0, Ordering::Relaxed);
    drop(token);

    let (validated, failure_code, stop_code) = match result {
        Ok(reading) => {
            let content = serde_json::to_string(&reading)
                .map_err(|_| "validated reading serialization failed".to_string())?;
            services.repository.transition_reading_turn(
                &session_id,
                &turn.id,
                &content,
                "COMPLETE",
            )?;
            record["status"] = json!("COMPLETE");
            record["failureCode"] = Value::Null;
            record["modelOutput"] = serde_json::to_value(reading)
                .map_err(|_| "validated reading result serialization failed".to_string())?;
            (true, None, None)
        }
        Err(error) => {
            let code = inference_error_code(&error);
            let state = if error == InferenceError::Cancelled {
                "cancelled"
            } else {
                "failed"
            };
            let status = if error == InferenceError::Cancelled {
                "CANCELLED"
            } else {
                "FAILED_RETRYABLE"
            };
            services.repository.transition_reading_turn(
                &session_id,
                &turn.id,
                &json!({"stage":state,"failureCode":code}).to_string(),
                status,
            )?;
            (false, Some(code), should_stop_batch(&error).then_some(code))
        }
    };
    record["status"] = json!(if validated { "COMPLETE" } else { "FAILED" });
    record["failureCode"] = failure_code.map_or(Value::Null, |code| json!(code));
    record["requestAttempts"] = json!(attempts);
    record["latencyMs"] = json!(latency_ms);
    if !validated {
        record["modelOutput"] = Value::Null;
    }
    write_jsonl(output, &record)?;
    println!(
        "  {} status={} attempts={} latencyMs={}",
        case_id,
        if validated {
            "COMPLETE"
        } else {
            failure_code.unwrap_or("FAILED")
        },
        attempts,
        latency_ms
    );
    Ok(CaseOutcome {
        session_id,
        validated,
        request_attempts: attempts,
        stop_code,
    })
}

async fn run_followup_case(
    case: &FollowupCase,
    session_id: &str,
    source_case: &ReadingCase,
    services: &CaseServices<'_>,
    output: &mut BufWriter<File>,
) -> Result<(bool, usize, Option<&'static str>), String> {
    let session = services
        .repository
        .get_session(session_id)?
        .ok_or_else(|| "follow-up source session is missing".to_string())?;
    if session.state != "READING_READY" {
        return Err("follow-up source reading is not complete".into());
    }
    let request_id = Uuid::new_v4().to_string();
    let (user_turn, assistant_turn) =
        services
            .repository
            .begin_followup(session_id, &request_id, &case.question, MODEL_SLUG)?;
    services.repository.transition_followup_turn(
        session_id,
        &assistant_turn.id,
        r#"{"stage":"streaming"}"#,
        "STREAMING",
    )?;
    let session = services
        .repository
        .get_session(session_id)?
        .ok_or_else(|| "follow-up temporary session was not persisted".to_string())?;
    let reveal = frozen_reveal(&source_case.question, source_case, services.cards_by_id)?;
    let mut record = case_common_record(CaseRecordInput {
        case_type: "followup",
        case_id: &case.id,
        question: &case.question,
        session_id,
        request_id: &request_id,
        cards: &reveal.cards,
        dataset_version: services.dataset_version,
        prompt_version: services.prompt_version,
    });
    record["readingCaseId"] = json!(case.reading_case_id);
    record["replyToTurnId"] = json!(user_turn.id);

    let token = match services.auth.access_token(services.store).await {
        Ok(token) => Zeroizing::new(token),
        Err(_) => {
            let code = "authorization_unavailable";
            services.repository.transition_followup_turn(
                session_id,
                &assistant_turn.id,
                &json!({"stage":"followup_failed","failureCode":code}).to_string(),
                "FAILED_RETRYABLE",
            )?;
            record["status"] = json!("FAILED");
            record["failureCode"] = json!(code);
            record["requestAttempts"] = json!(0);
            record["latencyMs"] = json!(0);
            record["modelOutput"] = Value::Null;
            write_jsonl(output, &record)?;
            return Ok((false, 0, Some(code)));
        }
    };

    PROVIDER_REQUEST_ATTEMPTS.store(0, Ordering::Relaxed);
    let started = Instant::now();
    let (_cancel_sender, cancellation) = watch::channel(false);
    let result = generate_followup_response(
        &session,
        &user_turn,
        &case.question,
        MODEL_SLUG,
        PROXY_MODE,
        &token,
        cancellation,
    )
    .await;
    let latency_ms = started.elapsed().as_millis();
    let attempts = PROVIDER_REQUEST_ATTEMPTS.swap(0, Ordering::Relaxed);
    drop(token);

    let (validated, failure_code, stop_code) = match result {
        Ok(response) => {
            let content = serde_json::to_string(&response)
                .map_err(|_| "validated follow-up serialization failed".to_string())?;
            services.repository.transition_followup_turn(
                session_id,
                &assistant_turn.id,
                &content,
                "COMPLETE",
            )?;
            record["modelOutput"] = serde_json::to_value(response)
                .map_err(|_| "validated follow-up result serialization failed".to_string())?;
            (true, None, None)
        }
        Err(error) => {
            let code = error.code();
            let status = if matches!(
                &error,
                FollowupFailure::Inference(InferenceError::Cancelled)
            ) {
                "CANCELLED"
            } else {
                "FAILED_RETRYABLE"
            };
            services.repository.transition_followup_turn(
                session_id,
                &assistant_turn.id,
                &error.turn_content("followup_failed"),
                status,
            )?;
            let stop = match &error {
                FollowupFailure::Inference(inference) => {
                    should_stop_batch(inference).then_some(code)
                }
                _ => None,
            };
            (false, Some(code), stop)
        }
    };
    record["status"] = json!(if validated { "COMPLETE" } else { "FAILED" });
    record["failureCode"] = failure_code.map_or(Value::Null, |code| json!(code));
    record["requestAttempts"] = json!(attempts);
    record["latencyMs"] = json!(latency_ms);
    if !validated {
        record["modelOutput"] = Value::Null;
    }
    write_jsonl(output, &record)?;
    println!(
        "  {} status={} attempts={} latencyMs={}",
        case.id,
        if validated {
            "COMPLETE"
        } else {
            failure_code.unwrap_or("FAILED")
        },
        attempts,
        latency_ms
    );
    Ok((validated, attempts, stop_code))
}

fn frozen_reveal(
    question: &str,
    source_case: &ReadingCase,
    cards_by_id: &HashMap<String, TarotCard>,
) -> Result<RevealSnapshot, String> {
    if source_case.deck_version != TAROT_DATASET_VERSION {
        return Err(format!(
            "{} references a different deck version",
            source_case.id
        ));
    }
    if validate_question(question)? != question {
        return Err(format!("{} question is not normalized", source_case.id));
    }
    if source_case.spread.len() != 3 {
        return Err(format!(
            "{} does not contain exactly three cards",
            source_case.id
        ));
    }
    let expected_slots = ["present", "obstacle", "tendency"];
    let mut unique_cards = HashSet::new();
    let mut cards = Vec::with_capacity(3);
    for (index, expected_slot) in expected_slots.iter().enumerate() {
        let input = &source_case.spread[index];
        if input.slot != *expected_slot
            || !matches!(input.orientation.as_str(), "upright" | "reversed")
            || !unique_cards.insert(input.card_id.as_str())
        {
            return Err(format!("{} has an invalid frozen spread", source_case.id));
        }
        let card = cards_by_id
            .get(&input.card_id)
            .ok_or_else(|| format!("{} references an unknown runtime card", source_case.id))?;
        let expected_keywords = if input.orientation == "upright" {
            &card.upright_keywords
        } else {
            &card.reversed_keywords
        };
        if card.name_zh != input.name_zh
            || card.name_en != input.name_en
            || expected_keywords != &input.keywords
        {
            return Err(format!(
                "{} card snapshot differs from runtime deck",
                source_case.id
            ));
        }
        cards.push(DrawnCard {
            slot: input.slot.clone(),
            deck_card_id: input.card_id.clone(),
            orientation: input.orientation.clone(),
            visual_order: index as u8,
            card: card.clone(),
        });
    }
    Ok(RevealSnapshot {
        draw_id: Uuid::new_v4().to_string(),
        dataset_version: TAROT_DATASET_VERSION.into(),
        question: question.to_owned(),
        cards,
    })
}

fn case_common_record(input: CaseRecordInput<'_>) -> Value {
    json!({
        "recordType":"case",
        "caseType":input.case_type,
        "caseId":input.case_id,
        "sessionId":input.session_id,
        "requestId":input.request_id,
        "question":input.question,
        "spread":input.cards.iter().map(|card| json!({
            "slot":card.slot,
            "cardId":card.deck_card_id,
            "orientation":card.orientation,
            "nameZh":card.card.name_zh,
            "nameEn":card.card.name_en,
            "keywords":if card.orientation == "upright" { &card.card.upright_keywords } else { &card.card.reversed_keywords }
        })).collect::<Vec<_>>(),
        "modelId":MODEL_SLUG,
        "modelEffort":MODEL_EFFORT,
        "datasetVersion":input.dataset_version,
        "promptVersion":input.prompt_version
    })
}

fn exact_luna_model(catalog: &[AvailableModel]) -> Result<&AvailableModel, String> {
    let model = catalog
        .iter()
        .find(|model| model.slug == MODEL_SLUG)
        .ok_or_else(|| "exact gpt-6-luna slug is absent from the authorized account catalog; no fallback was selected".to_string())?;
    let display_name = model.display_name.to_ascii_lowercase();
    if !display_name.contains('6') || !display_name.contains("luna") {
        return Err(
            "catalog display name does not confirm GPT-6-Luna; no inference requests were sent"
                .into(),
        );
    }
    Ok(model)
}

fn require_batch_approval() -> Result<(), String> {
    if std::env::var(APPROVAL_ENV).ok().as_deref() == Some(APPROVAL_VALUE) {
        Ok(())
    } else {
        Err("explicit 110-case GPT-6-Luna batch authorization marker is missing".into())
    }
}

async fn require_luna_catalog_model(
    auth: &ChatGptAuthManager,
    store: &CredentialStore,
) -> Result<AvailableModel, String> {
    let token = Zeroizing::new(
        auth.access_token(store)
            .await
            .map_err(|_| "official account authorization is unavailable")?,
    );
    let catalog = list_models(&token, Duration::from_secs(60), PROXY_MODE)
        .await
        .map_err(|error| {
            format!(
                "model catalog preflight failed: {}",
                inference_error_code(&error)
            )
        })?;
    exact_luna_model(&catalog).cloned()
}

fn validate_evaluation_inputs(
    readings: &[ReadingCase],
    followups: &[FollowupCase],
    sensitive: &[SensitiveCase],
    cards_by_id: &HashMap<String, TarotCard>,
) -> Result<(), String> {
    if readings.len() != EXPECTED_READING_CASES
        || followups.len() != EXPECTED_FOLLOWUP_CASES
        || sensitive.len() != EXPECTED_SENSITIVE_CASES
    {
        return Err("frozen M8 input counts changed; no inference requests were sent".into());
    }
    let reading_ids: HashSet<&str> = readings.iter().map(|case| case.id.as_str()).collect();
    if reading_ids.len() != readings.len() {
        return Err("frozen M8 reading IDs are not unique; no inference requests were sent".into());
    }
    for case in readings {
        frozen_reveal(&case.question, case, cards_by_id)?;
    }
    for case in followups {
        if !reading_ids.contains(case.reading_case_id.as_str())
            || validate_question(&case.question)? != case.question
        {
            return Err(
                "frozen M8 follow-up input is invalid; no inference requests were sent".into(),
            );
        }
    }
    for case in sensitive {
        let Some(source) = readings
            .iter()
            .find(|reading| reading.id == case.reading_case_id)
        else {
            return Err("frozen M8 sensitive case references a missing reading; no inference requests were sent".into());
        };
        if validate_question(&case.question)? != case.question {
            return Err(
                "frozen M8 sensitive question is invalid; no inference requests were sent".into(),
            );
        }
        frozen_reveal(&case.question, source, cards_by_id)?;
    }
    Ok(())
}

fn app_credential_store() -> Result<CredentialStore, String> {
    let config: Value = serde_json::from_str(include_str!("../../../tauri.conf.json"))
        .map_err(|_| "application identifier could not be read")?;
    let identifier = config["identifier"]
        .as_str()
        .ok_or("application identifier is missing")?;
    let app_data =
        std::env::var_os("APPDATA").ok_or("Windows application data directory is unavailable")?;
    Ok(CredentialStore::new(
        PathBuf::from(app_data).join(identifier).join("chatgpt"),
    ))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn load_jsonl<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>, String> {
    let file = File::open(path).map_err(|_| "an M8 frozen input file is unavailable")?;
    BufReader::new(file)
        .lines()
        .enumerate()
        .filter_map(|(index, line)| match line {
            Ok(line) if line.trim().is_empty() => None,
            Ok(line) => Some(
                serde_json::from_str(&line)
                    .map_err(|_| format!("M8 frozen input line {} is invalid JSON", index + 1)),
            ),
            Err(_) => Some(Err(format!(
                "could not read M8 frozen input line {}",
                index + 1
            ))),
        })
        .collect()
}

fn fingerprint_files(root: &Path, files: &[&str]) -> Result<String, String> {
    let mut digest = Sha256::new();
    for relative_path in files {
        let bytes = fs::read(root.join(relative_path))
            .map_err(|_| format!("could not fingerprint required file {relative_path}"))?;
        digest.update(relative_path.as_bytes());
        digest.update([0]);
        digest.update(bytes);
        digest.update([0]);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn write_jsonl(output: &mut BufWriter<File>, record: &Value) -> Result<(), String> {
    serde_json::to_writer(&mut *output, record)
        .map_err(|_| "could not serialize an M8 evaluation record".to_string())?;
    output
        .write_all(b"\n")
        .and_then(|()| output.flush())
        .and_then(|()| output.get_ref().sync_data())
        .map_err(|_| "could not persist an M8 evaluation record".to_string())
}

fn inference_error_code(error: &InferenceError) -> &'static str {
    match error {
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
    }
}

fn should_stop_batch(error: &InferenceError) -> bool {
    matches!(
        error,
        InferenceError::Unauthenticated
            | InferenceError::ExpiredOrRevoked
            | InferenceError::Timeout
            | InferenceError::ProxyOrNetwork
            | InferenceError::ModelUnavailable
            | InferenceError::PlanLimit
            | InferenceError::Cancelled
            | InferenceError::Server
    )
}

#[cfg(test)]
mod failure_code_tests {
    use super::inference_error_code;
    use crate::auth::inference::InferenceError;

    #[test]
    fn structured_output_and_stream_failures_have_distinct_safe_codes() {
        assert_eq!(
            inference_error_code(&InferenceError::InvalidStructuredOutput(
                "frozen_card_mismatch"
            )),
            "frozen_card_mismatch"
        );
        assert_eq!(
            inference_error_code(&InferenceError::InvalidResponse),
            "invalid_response_stream"
        );
    }
}
