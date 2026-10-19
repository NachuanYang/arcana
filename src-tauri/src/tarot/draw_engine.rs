use super::deck::{TarotCard, TarotDeck, TAROT_DATASET_VERSION};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const BACK_IMAGE_PATH: &str = "/assets/tarot/back.svg";
const DRAW_BACK_COUNT: u8 = 12;
const SLOT_NAMES: [&str; 3] = ["present", "obstacle", "tendency"];
const MIN_QUESTION_CHARACTERS: usize = 5;
const MAX_QUESTION_CHARACTERS: usize = 220;

pub trait RandomSource: Send + Sync {
    fn fill(&self, destination: &mut [u8]) -> Result<(), String>;
}

pub struct OperatingSystemRandom(SystemRandom);

impl Default for OperatingSystemRandom {
    fn default() -> Self {
        Self(SystemRandom::new())
    }
}

impl RandomSource for OperatingSystemRandom {
    fn fill(&self, destination: &mut [u8]) -> Result<(), String> {
        self.0
            .fill(destination)
            .map_err(|_| "Operating system random source failed".to_string())
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct BackPosition {
    pub position: u8,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DrawOffer {
    pub draw_id: String,
    pub question: String,
    pub backs: Vec<BackPosition>,
    pub back_image_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DrawnCard {
    pub slot: String,
    pub deck_card_id: String,
    pub orientation: String,
    pub visual_order: u8,
    pub card: TarotCard,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RevealSnapshot {
    pub draw_id: String,
    pub dataset_version: String,
    pub question: String,
    pub cards: Vec<DrawnCard>,
}

#[derive(Clone)]
struct HiddenCard {
    card: TarotCard,
    orientation: &'static str,
}

struct PendingDraw {
    question: String,
    cards: Vec<HiddenCard>,
    revealed_positions: Option<Vec<u8>>,
    snapshot: Option<RevealSnapshot>,
}

pub struct TarotDrawEngine {
    deck: TarotDeck,
    pending: Mutex<HashMap<String, PendingDraw>>,
    random: Arc<dyn RandomSource>,
}

impl TarotDrawEngine {
    pub fn new(deck: TarotDeck) -> Self {
        Self::with_random(deck, Arc::new(OperatingSystemRandom::default()))
    }

    pub fn with_random(deck: TarotDeck, random: Arc<dyn RandomSource>) -> Self {
        Self {
            deck,
            pending: Mutex::new(HashMap::new()),
            random,
        }
    }

    pub fn create_offer(&self, question: &str) -> Result<DrawOffer, String> {
        let question = validate_question(question)?;
        let mut shuffled = self.deck.cards.clone();
        fisher_yates(&mut shuffled, self.random.as_ref())?;
        let mut hidden = Vec::with_capacity(DRAW_BACK_COUNT as usize);
        for card in shuffled.into_iter().take(DRAW_BACK_COUNT as usize) {
            let orientation = if random_below(2, self.random.as_ref())? == 0 {
                "upright"
            } else {
                "reversed"
            };
            hidden.push(HiddenCard { card, orientation });
        }
        let draw_id = Uuid::new_v4().to_string();
        let pending = PendingDraw {
            question: question.clone(),
            cards: hidden,
            revealed_positions: None,
            snapshot: None,
        };
        self.pending
            .lock()
            .map_err(|_| "Draw engine state is unavailable".to_string())?
            .insert(draw_id.clone(), pending);
        Ok(DrawOffer {
            draw_id,
            question,
            backs: (0..DRAW_BACK_COUNT)
                .map(|position| BackPosition { position })
                .collect(),
            back_image_path: BACK_IMAGE_PATH.into(),
        })
    }

    pub fn catalog(&self) -> Vec<TarotCard> {
        self.deck.cards.clone()
    }

    pub fn reveal(
        &self,
        draw_id: &str,
        selected_positions: &[u8],
    ) -> Result<RevealSnapshot, String> {
        validate_selection(selected_positions)?;
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| "Draw engine state is unavailable".to_string())?;
        let draw = pending
            .get_mut(draw_id)
            .ok_or_else(|| "Draw does not exist or has been discarded".to_string())?;
        if let Some(snapshot) = &draw.snapshot {
            if draw.revealed_positions.as_deref() == Some(selected_positions) {
                return Ok(snapshot.clone());
            }
            return Err("This draw has already been revealed with a different selection".into());
        }
        let mut cards = Vec::with_capacity(3);
        for (visual_order, position) in selected_positions.iter().copied().enumerate() {
            let hidden = draw
                .cards
                .get(position as usize)
                .ok_or_else(|| "Selected position is outside the 12 card backs".to_string())?;
            cards.push(DrawnCard {
                slot: SLOT_NAMES[visual_order].into(),
                deck_card_id: hidden.card.id.clone(),
                orientation: hidden.orientation.into(),
                visual_order: visual_order as u8,
                card: hidden.card.clone(),
            });
        }
        let snapshot = RevealSnapshot {
            draw_id: draw_id.into(),
            dataset_version: TAROT_DATASET_VERSION.into(),
            question: draw.question.clone(),
            cards,
        };
        draw.revealed_positions = Some(selected_positions.to_vec());
        draw.snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }

    pub fn discard(&self, draw_id: &str) -> bool {
        self.pending
            .lock()
            .map(|mut pending| pending.remove(draw_id).is_some())
            .unwrap_or(false)
    }
}

pub fn validate_question(question: &str) -> Result<String, String> {
    let normalized = question.trim();
    let effective_characters = normalized
        .chars()
        .filter(|character| !character.is_whitespace())
        .count();
    if !(MIN_QUESTION_CHARACTERS..=MAX_QUESTION_CHARACTERS).contains(&effective_characters) {
        return Err("请输入5–220个有效字符的问题，空格不计。".into());
    }
    Ok(normalized.to_owned())
}

fn validate_selection(selected_positions: &[u8]) -> Result<(), String> {
    if selected_positions.len() != 3 {
        return Err("Select exactly three card backs before revealing".into());
    }
    let mut seen = [false; DRAW_BACK_COUNT as usize];
    for position in selected_positions {
        let index = *position as usize;
        if index >= seen.len() {
            return Err("Selected position is outside the 12 card backs".into());
        }
        if seen[index] {
            return Err("A card back cannot be selected more than once".into());
        }
        seen[index] = true;
    }
    Ok(())
}

fn fisher_yates<T>(values: &mut [T], random: &dyn RandomSource) -> Result<(), String> {
    for upper in (1..values.len()).rev() {
        let index = random_below((upper + 1) as u64, random)? as usize;
        values.swap(upper, index);
    }
    Ok(())
}

fn random_below(bound: u64, random: &dyn RandomSource) -> Result<u64, String> {
    if bound == 0 {
        return Err("Random bound must be positive".into());
    }
    let reject_below = bound.wrapping_neg() % bound;
    loop {
        let mut bytes = [0u8; 8];
        random.fill(&mut bytes)?;
        let sample = u64::from_le_bytes(bytes);
        if sample >= reject_below {
            return Ok(sample % bound);
        }
    }
}

#[tauri::command]
pub fn create_tarot_draw(
    question: String,
    engine: tauri::State<'_, TarotDrawEngine>,
) -> Result<DrawOffer, String> {
    engine.create_offer(&question)
}

#[tauri::command]
pub fn list_tarot_cards(engine: tauri::State<'_, TarotDrawEngine>) -> Vec<TarotCard> {
    engine.catalog()
}

#[tauri::command(rename_all = "camelCase")]
pub fn reveal_tarot_draw(
    draw_id: String,
    selected_positions: Vec<u8>,
    engine: tauri::State<'_, TarotDrawEngine>,
    repository: tauri::State<'_, std::sync::Arc<crate::persistence::SessionRepository>>,
) -> Result<RevealSnapshot, String> {
    let snapshot = engine.reveal(&draw_id, &selected_positions)?;
    repository.persist_reveal(&snapshot)?;
    Ok(snapshot)
}

#[tauri::command(rename_all = "camelCase")]
pub fn discard_tarot_draw(draw_id: String, engine: tauri::State<'_, TarotDrawEngine>) -> bool {
    engine.discard(&draw_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct QueueRandom(Mutex<VecDeque<u64>>);

    impl QueueRandom {
        fn repeating(value: u64) -> Self {
            Self(Mutex::new(std::iter::repeat_n(value, 20_000).collect()))
        }
        fn sequence(values: impl IntoIterator<Item = u64>) -> Self {
            Self(Mutex::new(values.into_iter().collect()))
        }
    }

    impl RandomSource for QueueRandom {
        fn fill(&self, destination: &mut [u8]) -> Result<(), String> {
            if destination.len() != 8 {
                return Err("test source expects 8 byte samples".into());
            }
            let value = self
                .0
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "test random sequence exhausted".to_string())?;
            destination.copy_from_slice(&value.to_le_bytes());
            Ok(())
        }
    }

    struct FailedRandom;
    impl RandomSource for FailedRandom {
        fn fill(&self, _: &mut [u8]) -> Result<(), String> {
            Err("test OS random failure".into())
        }
    }

    fn engine_with(random: Arc<dyn RandomSource>) -> TarotDrawEngine {
        TarotDrawEngine::with_random(TarotDeck::load_bundled().unwrap(), random)
    }
    fn default_engine() -> TarotDrawEngine {
        TarotDrawEngine::new(TarotDeck::load_bundled().unwrap())
    }
    fn offer(engine: &TarotDrawEngine) -> DrawOffer {
        engine.create_offer("项目如何走").unwrap()
    }

    #[test]
    fn question_validator_accepts_five_effective_characters() {
        assert_eq!(validate_question("项目如何走").unwrap(), "项目如何走");
    }

    #[test]
    fn question_validator_rejects_four_effective_characters() {
        assert!(validate_question("项目如何").is_err());
    }

    #[test]
    fn question_validator_accepts_220_effective_characters() {
        assert_eq!(
            validate_question(&"问".repeat(220))
                .unwrap()
                .chars()
                .count(),
            220
        );
    }

    #[test]
    fn question_validator_rejects_221_effective_characters() {
        assert!(validate_question(&"问".repeat(221)).is_err());
    }

    #[test]
    fn question_validator_rejects_whitespace_only_input() {
        assert!(validate_question(" \t\n ").is_err());
    }

    #[test]
    fn question_length_excludes_internal_whitespace() {
        assert_eq!(
            validate_question("项 目 如 何 走").unwrap(),
            "项 目 如 何 走"
        );
    }

    #[test]
    fn question_validator_trims_outer_whitespace() {
        assert_eq!(validate_question("  项目如何走  ").unwrap(), "项目如何走");
    }

    #[test]
    fn offer_and_reveal_keep_the_normalized_question() {
        let engine = default_engine();
        let draw = engine.create_offer("  项目如何走  ").unwrap();
        assert_eq!(draw.question, "项目如何走");
        assert_eq!(
            engine.reveal(&draw.draw_id, &[0, 1, 2]).unwrap().question,
            "项目如何走"
        );
    }

    #[test]
    fn invalid_question_does_not_consume_randomness_or_create_a_draw() {
        let random = Arc::new(QueueRandom::repeating(u64::MAX));
        let engine = engine_with(random.clone());
        assert!(engine.create_offer("短").is_err());
        assert!(engine.pending.lock().unwrap().is_empty());
        assert_eq!(random.0.lock().unwrap().len(), 20_000);
    }

    #[test]
    fn offer_exposes_exactly_twelve_back_positions() {
        assert_eq!(offer(&default_engine()).backs.len(), 12);
    }
    #[test]
    fn offer_positions_are_zero_through_eleven() {
        assert_eq!(
            offer(&default_engine())
                .backs
                .iter()
                .map(|b| b.position)
                .collect::<Vec<_>>(),
            (0..12).collect::<Vec<_>>()
        );
    }
    #[test]
    fn all_positions_use_the_same_original_back_art() {
        assert_eq!(offer(&default_engine()).back_image_path, BACK_IMAGE_PATH);
    }
    #[test]
    fn offer_has_an_opaque_draw_id() {
        let value = serde_json::to_value(offer(&default_engine())).unwrap();
        assert!(value["drawId"].as_str().is_some());
    }

    #[test]
    fn local_catalog_returns_all_78_validated_cards() {
        assert_eq!(default_engine().catalog().len(), 78);
    }
    #[test]
    fn offer_serialization_contains_no_card_id_or_card_name() {
        let value = serde_json::to_string(&offer(&default_engine())).unwrap();
        assert!(
            !value.contains("major_")
                && !value.contains("minor_")
                && !value.contains("The Fool")
                && !value.contains("愚者")
        );
    }
    #[test]
    fn offer_serialization_contains_no_orientation() {
        let value = serde_json::to_string(&offer(&default_engine())).unwrap();
        assert!(!value.contains("upright") && !value.contains("reversed"));
    }
    #[test]
    fn repeated_offer_requests_get_distinct_ids() {
        let engine = default_engine();
        assert_ne!(offer(&engine).draw_id, offer(&engine).draw_id);
    }
    #[test]
    fn draw_failure_surfaces_os_random_error() {
        assert!(engine_with(Arc::new(FailedRandom))
            .create_offer("项目如何走")
            .unwrap_err()
            .contains("random"));
    }
    #[test]
    fn random_below_rejects_modulo_biased_low_values() {
        let random = QueueRandom::sequence([0, 4]);
        assert_eq!(random_below(3, &random).unwrap(), 1);
    }
    #[test]
    fn random_below_rejects_zero_bound() {
        assert!(random_below(0, &QueueRandom::repeating(1)).is_err());
    }
    #[test]
    fn fisher_yates_preserves_all_78_cards() {
        let mut cards = TarotDeck::load_bundled().unwrap().cards;
        fisher_yates(&mut cards, &QueueRandom::repeating(u64::MAX)).unwrap();
        assert_eq!(cards.len(), 78);
    }
    #[test]
    fn fisher_yates_keeps_card_ids_unique() {
        let mut cards = TarotDeck::load_bundled().unwrap().cards;
        fisher_yates(&mut cards, &QueueRandom::repeating(0x9e3779b97f4a7c15)).unwrap();
        assert_eq!(
            cards
                .iter()
                .map(|c| &c.id)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            78
        );
    }

    #[test]
    fn ten_thousand_os_random_draws_preserve_spread_invariants() {
        const DRAW_COUNT: usize = 10_000;

        let engine = default_engine();
        let valid_card_ids: std::collections::HashSet<_> = engine
            .deck
            .cards
            .iter()
            .map(|card| card.id.as_str())
            .collect();
        let mut draw_ids = std::collections::HashSet::with_capacity(DRAW_COUNT);

        for _ in 0..DRAW_COUNT {
            let draw = offer(&engine);
            assert!(
                draw_ids.insert(draw.draw_id.clone()),
                "draw IDs must be unique"
            );

            let snapshot = engine.reveal(&draw.draw_id, &[0, 4, 8]).unwrap();
            assert_eq!(
                snapshot
                    .cards
                    .iter()
                    .map(|card| card.slot.as_str())
                    .collect::<Vec<_>>(),
                SLOT_NAMES
            );
            assert_eq!(snapshot.cards.len(), 3);
            assert_eq!(
                snapshot
                    .cards
                    .iter()
                    .map(|card| card.visual_order)
                    .collect::<Vec<_>>(),
                [0, 1, 2]
            );
            assert_eq!(
                snapshot
                    .cards
                    .iter()
                    .map(|card| card.deck_card_id.as_str())
                    .collect::<std::collections::HashSet<_>>()
                    .len(),
                3,
                "a revealed spread must not contain duplicate cards"
            );
            assert!(snapshot.cards.iter().all(|card| {
                valid_card_ids.contains(card.deck_card_id.as_str())
                    && matches!(card.orientation.as_str(), "upright" | "reversed")
            }));
            assert!(engine.discard(&draw.draw_id));
        }

        assert_eq!(draw_ids.len(), DRAW_COUNT);
        assert!(engine.pending.lock().unwrap().is_empty());
    }

    #[test]
    fn every_offer_maps_twelve_distinct_cards_internally() {
        let engine = engine_with(Arc::new(QueueRandom::repeating(u64::MAX)));
        let offer = offer(&engine);
        let pending = engine.pending.lock().unwrap();
        let hidden_cards = &pending[&offer.draw_id].cards;
        assert_eq!(hidden_cards.len(), 12);
        assert_eq!(
            hidden_cards
                .iter()
                .map(|c| &c.card.id)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            12
        );
    }

    #[test]
    fn all_twelve_hidden_orientations_are_valid_independent_card_values() {
        let engine = engine_with(Arc::new(QueueRandom::repeating(u64::MAX)));
        let offer = offer(&engine);
        let pending = engine.pending.lock().unwrap();
        assert!(pending[&offer.draw_id]
            .cards
            .iter()
            .all(|card| matches!(card.orientation, "upright" | "reversed")));
        assert_eq!(pending[&offer.draw_id].cards.len(), 12);
    }
    #[test]
    fn reveal_rejects_less_than_three_positions() {
        let engine = default_engine();
        let draw = offer(&engine);
        assert!(engine.reveal(&draw.draw_id, &[0, 1]).is_err());
    }
    #[test]
    fn reveal_rejects_more_than_three_positions() {
        let engine = default_engine();
        let draw = offer(&engine);
        assert!(engine.reveal(&draw.draw_id, &[0, 1, 2, 3]).is_err());
    }
    #[test]
    fn reveal_rejects_duplicate_positions() {
        let engine = default_engine();
        let draw = offer(&engine);
        assert!(engine.reveal(&draw.draw_id, &[0, 0, 2]).is_err());
    }
    #[test]
    fn reveal_rejects_position_outside_twelve_backs() {
        let engine = default_engine();
        let draw = offer(&engine);
        assert!(engine.reveal(&draw.draw_id, &[0, 1, 12]).is_err());
    }
    #[test]
    fn reveal_accepts_the_twelfth_back() {
        let engine = default_engine();
        let draw = offer(&engine);
        assert!(engine.reveal(&draw.draw_id, &[0, 1, 11]).is_ok());
    }
    #[test]
    fn invalid_reveal_does_not_consume_draw() {
        let engine = default_engine();
        let draw = offer(&engine);
        assert!(engine.reveal(&draw.draw_id, &[0, 0, 2]).is_err());
        assert!(engine.reveal(&draw.draw_id, &[0, 1, 2]).is_ok());
    }
    #[test]
    fn reveal_assigns_fixed_present_obstacle_tendency_slots() {
        let engine = default_engine();
        let draw = offer(&engine);
        let snapshot = engine.reveal(&draw.draw_id, &[7, 2, 5]).unwrap();
        assert_eq!(
            snapshot
                .cards
                .iter()
                .map(|c| c.slot.as_str())
                .collect::<Vec<_>>(),
            ["present", "obstacle", "tendency"]
        );
    }
    #[test]
    fn reveal_preserves_selected_visual_order() {
        let engine = default_engine();
        let draw = offer(&engine);
        let snapshot = engine.reveal(&draw.draw_id, &[7, 2, 5]).unwrap();
        assert_eq!(
            snapshot
                .cards
                .iter()
                .map(|c| c.visual_order)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
    }
    #[test]
    fn reveal_uses_verified_dataset_version() {
        let engine = default_engine();
        let draw = offer(&engine);
        assert_eq!(
            engine
                .reveal(&draw.draw_id, &[0, 1, 2])
                .unwrap()
                .dataset_version,
            TAROT_DATASET_VERSION
        );
    }
    #[test]
    fn repeated_reveal_is_idempotent() {
        let engine = default_engine();
        let draw = offer(&engine);
        let one = engine.reveal(&draw.draw_id, &[0, 1, 2]).unwrap();
        let two = engine.reveal(&draw.draw_id, &[0, 1, 2]).unwrap();
        assert_eq!(one, two);
    }
    #[test]
    fn reveal_rejects_changed_selection_after_freezing() {
        let engine = default_engine();
        let draw = offer(&engine);
        engine.reveal(&draw.draw_id, &[0, 1, 2]).unwrap();
        assert!(engine
            .reveal(&draw.draw_id, &[3, 4, 5])
            .unwrap_err()
            .contains("different selection"));
    }
    #[test]
    fn reveal_rejects_same_cards_reordered_after_freezing() {
        let engine = default_engine();
        let draw = offer(&engine);
        engine.reveal(&draw.draw_id, &[0, 1, 2]).unwrap();
        assert!(engine.reveal(&draw.draw_id, &[2, 1, 0]).is_err());
    }
    #[test]
    fn selected_position_maps_to_its_fixed_hidden_card() {
        let engine = default_engine();
        let draw = offer(&engine);
        let all = engine.pending.lock().unwrap();
        let expected = all[&draw.draw_id].cards[4].card.id.clone();
        drop(all);
        let snapshot = engine.reveal(&draw.draw_id, &[4, 1, 7]).unwrap();
        assert_eq!(snapshot.cards[0].deck_card_id, expected);
    }
    #[test]
    fn reveal_snapshot_contains_three_distinct_cards() {
        let engine = default_engine();
        let draw = offer(&engine);
        let snapshot = engine.reveal(&draw.draw_id, &[3, 0, 8]).unwrap();
        assert_eq!(
            snapshot
                .cards
                .iter()
                .map(|c| &c.deck_card_id)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            3
        );
    }
    #[test]
    fn reveal_snapshots_include_card_keyword_context() {
        let engine = default_engine();
        let draw = offer(&engine);
        let snapshot = engine.reveal(&draw.draw_id, &[0, 1, 2]).unwrap();
        assert!(snapshot
            .cards
            .iter()
            .all(|c| !c.card.upright_keywords.is_empty() && !c.card.reversed_keywords.is_empty()));
    }
    #[test]
    fn orientations_are_generated_before_reveal_but_not_returned_early() {
        let engine = engine_with(Arc::new(QueueRandom::repeating(u64::MAX)));
        let draw = offer(&engine);
        let value = serde_json::to_string(&draw).unwrap();
        assert!(!value.contains("reversed") && !value.contains("upright"));
        assert!(engine
            .reveal(&draw.draw_id, &[0, 1, 2])
            .unwrap()
            .cards
            .iter()
            .all(|c| c.orientation == "reversed"));
    }
    #[test]
    fn orientation_coin_flips_can_differ_independently() {
        let mut sequence = vec![u64::MAX; 77];
        sequence.extend([
            u64::MAX,
            u64::MAX - 1,
            u64::MAX,
            u64::MAX - 1,
            u64::MAX,
            u64::MAX - 1,
            u64::MAX,
            u64::MAX - 1,
            u64::MAX,
            u64::MAX - 1,
            u64::MAX,
            u64::MAX - 1,
        ]);
        let engine = engine_with(Arc::new(QueueRandom::sequence(sequence)));
        let draw = offer(&engine);
        let snapshot = engine.reveal(&draw.draw_id, &[0, 1, 2]).unwrap();
        assert_ne!(snapshot.cards[0].orientation, snapshot.cards[1].orientation);
    }
    #[test]
    fn discard_removes_unrevealed_draw() {
        let engine = default_engine();
        let draw = offer(&engine);
        assert!(engine.discard(&draw.draw_id));
        assert!(engine.reveal(&draw.draw_id, &[0, 1, 2]).is_err());
    }
    #[test]
    fn discard_returns_false_for_missing_draw() {
        assert!(!default_engine().discard("absent"));
    }
    #[test]
    fn reveal_rejects_unknown_draw_id() {
        assert!(default_engine().reveal("absent", &[0, 1, 2]).is_err());
    }
}
