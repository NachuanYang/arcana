use super::repository::SessionRepository;
use crate::tarot::deck::TarotCard;
use crate::tarot::draw_engine::{DrawnCard, RevealSnapshot};

fn snapshot(draw_id: &str) -> RevealSnapshot {
    let slots = ["present", "obstacle", "tendency"];
    RevealSnapshot {
        draw_id: draw_id.into(),
        dataset_version: "rws_pam_a_taionwc_v1".into(),
        question: "项目如何走".into(),
        cards: slots
            .iter()
            .enumerate()
            .map(|(index, slot)| DrawnCard {
                slot: (*slot).into(),
                deck_card_id: format!("major_{index:02}"),
                orientation: if index == 1 { "reversed" } else { "upright" }.into(),
                visual_order: index as u8,
                card: TarotCard {
                    id: format!("major_{index:02}"),
                    arcana: "major".into(),
                    number: Some(index as u8),
                    suit: None,
                    rank: None,
                    name_zh: ["愚者", "魔术师", "女祭司"][index].into(),
                    name_en: ["The Fool", "The Magician", "The High Priestess"][index].into(),
                    symbol: "✧".into(),
                    upright_keywords: vec!["启程".into()],
                    reversed_keywords: vec!["鲁莽".into()],
                    image_path: format!("/assets/tarot/rws/major_{index:02}.webp"),
                },
            })
            .collect(),
    }
}

#[test]
fn fresh_database_migrates_and_reopening_is_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("arcanum.sqlite3");

    let first = SessionRepository::open(&path).unwrap();
    assert_eq!(first.schema_version().unwrap(), 2);
    drop(first);

    let reopened = SessionRepository::open(&path).unwrap();
    assert_eq!(reopened.schema_version().unwrap(), 2);
}

#[test]
fn v1_database_upgrades_followup_states_without_losing_cards_or_foreign_keys() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("arcanum-v1.sqlite3");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .unwrap();
    connection
        .execute_batch(include_str!("../../migrations/0001_initial.sql"))
        .unwrap();
    connection.pragma_update(None, "user_version", 1).unwrap();
    connection
        .execute(
            "INSERT INTO tarot_sessions
             (id, question, spread_id, deck_version, state, created_at, revealed_at, updated_at)
             VALUES ('upgrade-session', '项目如何走', 'three_present_obstacle_tendency',
                     'rws_pam_a_taionwc_v1', 'READING_READY', '2026-10-12T00:00:00Z',
                     '2026-10-12T00:00:00Z', '2026-10-12T00:00:00Z')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO tarot_turns
             (id, session_id, turn_order, role, request_id, content_json, status, model_id, created_at)
             VALUES ('old-reading', 'upgrade-session', 1, 'assistant', 'old-request',
                     '{\"stance\":\"CAUTIOUS\"}', 'COMPLETE', 'gpt-test', '2026-10-12T00:01:00Z')",
            [],
        )
        .unwrap();
    for drawn in &snapshot("upgrade-session").cards {
        connection
            .execute(
                "INSERT INTO tarot_drawn_cards
                 (session_id, slot, card_id, orientation, visual_order, card_snapshot_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "upgrade-session",
                    drawn.slot,
                    drawn.deck_card_id,
                    drawn.orientation,
                    drawn.visual_order,
                    serde_json::to_string(drawn).unwrap()
                ],
            )
            .unwrap();
    }
    drop(connection);

    let migrated = SessionRepository::open(&path).unwrap();
    assert_eq!(migrated.schema_version().unwrap(), 2);
    let restored = migrated.get_session("upgrade-session").unwrap().unwrap();
    assert_eq!(restored.state, "READING_READY");
    assert_eq!(restored.reading_stance.as_deref(), Some("CAUTIOUS"));
    assert_eq!(restored.cards.len(), 3);
    assert_eq!(
        migrated
            .child_record_counts_for_test("upgrade-session")
            .unwrap(),
        (3, 1)
    );
    assert!(migrated.delete_session("upgrade-session").unwrap());
    assert_eq!(
        migrated
            .child_record_counts_for_test("upgrade-session")
            .unwrap(),
        (0, 0)
    );
}

#[test]
fn newer_database_schema_is_rejected_without_downgrade() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("arcanum.sqlite3");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.pragma_update(None, "user_version", 99).unwrap();
    drop(connection);

    let error = SessionRepository::open(&path).err().unwrap();
    assert!(error.contains("newer than this app supports"));
}

#[test]
fn database_constraints_reject_invalid_state_slot_orientation_json_and_orphan_cards() {
    let repository = SessionRepository::in_memory().unwrap();
    repository
        .persist_reveal(&snapshot("checked-session"))
        .unwrap();

    assert_eq!(
        repository
            .check_schema_constraints_for_test("checked-session")
            .unwrap(),
        [true, true, true, true, true]
    );
}

#[test]
fn reveal_is_saved_atomically_idempotently_and_survives_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("arcanum.sqlite3");
    let reveal = snapshot("stable-session");

    {
        let repository = SessionRepository::open(&path).unwrap();
        repository.persist_reveal(&reveal).unwrap();
        repository.persist_reveal(&reveal).unwrap();
        assert_eq!(
            repository
                .get_session("stable-session")
                .unwrap()
                .unwrap()
                .cards,
            reveal.cards
        );
    }

    let repository = SessionRepository::open(&path).unwrap();
    let restored = repository.get_session("stable-session").unwrap().unwrap();
    assert_eq!(restored.question, reveal.question);
    assert_eq!(restored.state, "REVEALED");
    assert_eq!(restored.cards, reveal.cards);
}

#[test]
fn conflicting_reveal_retry_is_rejected_without_overwriting_original_cards() {
    let repository = SessionRepository::in_memory().unwrap();
    let original = snapshot("stable-session");
    repository.persist_reveal(&original).unwrap();
    let mut changed = original.clone();
    changed.cards[0].orientation = "reversed".into();

    assert!(repository.persist_reveal(&changed).is_err());
    assert_eq!(
        repository
            .get_session("stable-session")
            .unwrap()
            .unwrap()
            .cards,
        original.cards
    );
}

#[test]
fn reveal_transaction_rolls_back_when_a_later_card_insert_fails() {
    let repository = SessionRepository::in_memory().unwrap();
    repository.fail_tendency_insert_for_test().unwrap();

    assert!(repository
        .persist_reveal(&snapshot("rolled-back-session"))
        .is_err());
    assert!(repository
        .get_session("rolled-back-session")
        .unwrap()
        .is_none());
    assert_eq!(
        repository
            .card_count_for_test("rolled-back-session")
            .unwrap(),
        0
    );
}

#[test]
fn malformed_persisted_card_json_fails_explicitly_in_detail_and_history_reads() {
    let repository = SessionRepository::in_memory().unwrap();
    repository
        .persist_reveal(&snapshot("malformed-session"))
        .unwrap();
    repository
        .corrupt_card_json_for_test("malformed-session")
        .unwrap();

    assert!(repository
        .get_session("malformed-session")
        .unwrap_err()
        .contains("malformed"));
    assert!(repository
        .list_sessions()
        .unwrap_err()
        .contains("malformed"));
}

#[test]
fn malformed_turn_json_fails_explicitly_in_session_and_turn_reads() {
    let repository = SessionRepository::in_memory().unwrap();
    repository
        .persist_reveal(&snapshot("malformed-turn-session"))
        .unwrap();
    repository
        .append_turn(
            "malformed-turn-session",
            Some("request-1"),
            "assistant",
            r#"{"text":"ok"}"#,
            "COMPLETE",
            Some("model-a"),
        )
        .unwrap();
    repository
        .corrupt_turn_json_for_test("malformed-turn-session")
        .unwrap();

    assert!(repository
        .get_session("malformed-turn-session")
        .unwrap_err()
        .contains("malformed"));
    assert!(repository
        .list_turns("malformed-turn-session")
        .unwrap_err()
        .contains("malformed"));
}

#[test]
fn sessions_are_listed_newest_first_and_deletion_cascades() {
    let repository = SessionRepository::in_memory().unwrap();
    repository
        .persist_reveal(&snapshot("older-session"))
        .unwrap();
    repository
        .persist_reveal(&snapshot("newer-session"))
        .unwrap();
    let turn = repository
        .append_turn(
            "older-session",
            Some("request-1"),
            "assistant",
            r#"{"text":"已完成"}"#,
            "COMPLETE",
            Some("model-a"),
        )
        .unwrap();
    assert_eq!(turn.turn_order, 1);

    let sessions = repository.list_sessions().unwrap();
    assert_eq!(
        sessions
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["newer-session", "older-session"]
    );
    assert_eq!(repository.list_turns("older-session").unwrap().len(), 1);
    assert!(repository.delete_session("older-session").unwrap());
    assert!(!repository.delete_session("older-session").unwrap());
    assert!(repository.get_session("older-session").unwrap().is_none());
    assert_eq!(
        repository
            .child_record_counts_for_test("older-session")
            .unwrap(),
        (0, 0)
    );
}

#[test]
fn turns_are_idempotent_and_interrupted_work_recovers_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("arcanum.sqlite3");
    let repository = SessionRepository::open(&path).unwrap();
    repository
        .persist_reveal(&snapshot("pending-session"))
        .unwrap();
    repository
        .set_session_state("pending-session", "READING_PENDING")
        .unwrap();
    let first = repository
        .append_turn(
            "pending-session",
            Some("same-request"),
            "assistant",
            r#"{"partial":"内容"}"#,
            "PENDING",
            Some("model-a"),
        )
        .unwrap();
    assert_eq!(
        repository
            .append_turn(
                "pending-session",
                Some("same-request"),
                "assistant",
                r#"{"partial":"内容"}"#,
                "PENDING",
                Some("model-a")
            )
            .unwrap(),
        first
    );
    drop(repository);

    let reopened = SessionRepository::open(&path).unwrap();
    let restored = reopened.get_session("pending-session").unwrap().unwrap();
    assert_eq!(restored.state, "READING_FAILED_RETRYABLE");
    assert_eq!(
        reopened.list_turns("pending-session").unwrap()[0].status,
        "FAILED_RETRYABLE"
    );
}

#[test]
fn turns_update_in_order_and_completed_content_cannot_be_replaced() {
    let repository = SessionRepository::in_memory().unwrap();
    repository.persist_reveal(&snapshot("session-a")).unwrap();
    let pending = repository
        .append_turn(
            "session-a",
            Some("request-a"),
            "assistant",
            r#"{"text":""}"#,
            "PENDING",
            Some("model-a"),
        )
        .unwrap();
    let streaming = repository
        .update_turn("session-a", &pending.id, r#"{"text":"部分"}"#, "STREAMING")
        .unwrap();
    assert_eq!(streaming.turn_order, 1);
    assert_eq!(streaming.status, "STREAMING");
    let complete = repository
        .update_turn("session-a", &pending.id, r#"{"text":"完整"}"#, "COMPLETE")
        .unwrap();
    assert_eq!(complete.content_json, r#"{"text":"完整"}"#);
    assert!(repository
        .update_turn("session-a", &pending.id, r#"{"text":"覆盖"}"#, "COMPLETE")
        .is_err());

    repository.persist_reveal(&snapshot("session-b")).unwrap();
    assert!(repository
        .update_turn("session-b", &pending.id, r#"{"text":"串会话"}"#, "COMPLETE")
        .is_err());
    assert_eq!(repository.list_turns("session-a").unwrap()[0], complete);
}

#[test]
fn reading_turn_and_session_status_transitions_are_atomic_and_scoped() {
    let repository = SessionRepository::in_memory().unwrap();
    repository
        .persist_reveal(&snapshot("reading-session"))
        .unwrap();

    let pending = repository
        .begin_reading_turn("reading-session", "request-1", "gpt-account-slug")
        .unwrap();
    assert_eq!(
        repository
            .get_session("reading-session")
            .unwrap()
            .unwrap()
            .state,
        "READING_PENDING"
    );
    assert_eq!(
        repository
            .begin_reading_turn("reading-session", "request-1", "gpt-account-slug")
            .unwrap(),
        pending
    );
    assert!(repository
        .begin_reading_turn("reading-session", "request-1", "another-model")
        .is_err());

    let streaming = repository
        .transition_reading_turn(
            "reading-session",
            &pending.id,
            r#"{"stage":"streaming"}"#,
            "STREAMING",
        )
        .unwrap();
    assert_eq!(streaming.status, "STREAMING");
    assert_eq!(
        repository
            .get_session("reading-session")
            .unwrap()
            .unwrap()
            .state,
        "READING_STREAMING"
    );

    let completed = repository
        .transition_reading_turn(
            "reading-session",
            &pending.id,
            r#"{"schemaVersion":1,"stance":"CAUTIOUS"}"#,
            "COMPLETE",
        )
        .unwrap();
    assert_eq!(completed.status, "COMPLETE");
    assert_eq!(
        repository
            .get_session("reading-session")
            .unwrap()
            .unwrap()
            .reading_stance
            .as_deref(),
        Some("CAUTIOUS")
    );
    assert_eq!(
        repository
            .get_session("reading-session")
            .unwrap()
            .unwrap()
            .state,
        "READING_READY"
    );
    assert!(repository
        .transition_reading_turn(
            "other-session",
            &pending.id,
            r#"{"text":"late"}"#,
            "COMPLETE"
        )
        .is_err());
}

fn ready_reading(repository: &SessionRepository, session_id: &str) {
    repository.persist_reveal(&snapshot(session_id)).unwrap();
    let pending = repository
        .begin_reading_turn(session_id, "first-reading", "gpt-test")
        .unwrap();
    repository
        .transition_reading_turn(
            session_id,
            &pending.id,
            r#"{"stage":"streaming"}"#,
            "STREAMING",
        )
        .unwrap();
    repository
        .transition_reading_turn(
            session_id,
            &pending.id,
            &format!(r#"{{"schemaVersion":1,"sessionId":"{session_id}","stance":"CAUTIOUS"}}"#),
            "COMPLETE",
        )
        .unwrap();
}

#[test]
fn followup_turns_are_atomic_scoped_and_retry_the_same_question_without_counting_twice() {
    let repository = SessionRepository::in_memory().unwrap();
    ready_reading(&repository, "followup-a");
    ready_reading(&repository, "followup-b");

    let (user, assistant) = repository
        .begin_followup("followup-a", "request-1", "为什么需要谨慎？", "gpt-test")
        .unwrap();
    assert_eq!(user.role, "user");
    assert_eq!(user.status, "COMPLETE");
    assert_eq!(assistant.role, "assistant");
    assert_eq!(assistant.status, "PENDING");
    assert_eq!(
        repository.get_session("followup-a").unwrap().unwrap().state,
        "FOLLOWUP_PENDING"
    );
    assert!(repository
        .begin_followup("followup-a", "request-1", "换成另一个问题", "gpt-test")
        .is_err());
    assert!(repository
        .transition_followup_turn(
            "followup-b",
            &assistant.id,
            r#"{"stage":"done"}"#,
            "COMPLETE"
        )
        .is_err());

    repository
        .transition_followup_turn(
            "followup-a",
            &assistant.id,
            r#"{"stage":"streaming"}"#,
            "STREAMING",
        )
        .unwrap();
    repository
        .transition_followup_turn(
            "followup-a",
            &assistant.id,
            r#"{"stage":"failed"}"#,
            "FAILED_RETRYABLE",
        )
        .unwrap();
    let (retried_user, retried_assistant) = repository
        .begin_followup("followup-a", "request-1", "为什么需要谨慎？", "gpt-test")
        .unwrap();
    assert_eq!(retried_user.id, user.id);
    assert_eq!(retried_assistant.id, assistant.id);
    assert_eq!(retried_assistant.status, "PENDING");
    assert_eq!(
        repository
            .get_session("followup-a")
            .unwrap()
            .unwrap()
            .turns
            .iter()
            .filter(|turn| turn.role == "user")
            .count(),
        1
    );
}

#[test]
fn followups_have_no_product_count_limit_and_interrupted_followup_recovers_to_retryable_reading() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("arcanum-followup.sqlite3");
    let repository = SessionRepository::open(&path).unwrap();
    ready_reading(&repository, "followup-limit");
    ready_reading(&repository, "followup-crash");

    for index in 1..=7 {
        let request_id = format!("followup-{index}");
        let question = format!("第{index}次追问要点是什么？");
        let (_, assistant) = repository
            .begin_followup("followup-limit", &request_id, &question, "gpt-test")
            .unwrap();
        repository
            .transition_followup_turn(
                "followup-limit",
                &assistant.id,
                r#"{"stage":"streaming"}"#,
                "STREAMING",
            )
            .unwrap();
        repository
            .transition_followup_turn(
                "followup-limit",
                &assistant.id,
                &format!(r#"{{"stage":"answer-{index}"}}"#),
                "COMPLETE",
            )
            .unwrap();
    }
    let (_, seventh_assistant) = repository
        .begin_followup(
            "followup-limit",
            "followup-8",
            "第八条追问仍可继续吗？",
            "gpt-test",
        )
        .unwrap();
    assert_eq!(seventh_assistant.status, "PENDING");
    assert_eq!(
        repository
            .get_session("followup-limit")
            .unwrap()
            .unwrap()
            .turns
            .len(),
        17
    );

    let (_, in_flight) = repository
        .begin_followup(
            "followup-crash",
            "restart-me",
            "重新打开后如何继续？",
            "gpt-test",
        )
        .unwrap();
    drop(repository);
    let reopened = SessionRepository::open(&path).unwrap();
    let recovered = reopened.get_session("followup-crash").unwrap().unwrap();
    assert_eq!(recovered.state, "READING_READY");
    assert_eq!(
        recovered
            .turns
            .iter()
            .find(|turn| turn.id == in_flight.id)
            .unwrap()
            .status,
        "FAILED_RETRYABLE"
    );
    let (retried_user, retried_assistant) = reopened
        .begin_followup(
            "followup-crash",
            "restart-me",
            "重新打开后如何继续？",
            "gpt-test",
        )
        .unwrap();
    assert_eq!(retried_user.role, "user");
    assert_eq!(retried_assistant.id, in_flight.id);
    assert_eq!(retried_assistant.status, "PENDING");
}
