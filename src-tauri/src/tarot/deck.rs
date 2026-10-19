use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const TAROT_DATASET_VERSION: &str = "rws_pam_a_taionwc_v1";
const RUNTIME_DATA: &str = include_str!("../../../data/tarot_78.json");

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TarotCard {
    pub id: String,
    pub arcana: String,
    #[serde(default)]
    pub number: Option<u8>,
    #[serde(default)]
    pub suit: Option<String>,
    #[serde(default)]
    pub rank: Option<String>,
    #[serde(rename = "name_zh")]
    pub name_zh: String,
    #[serde(rename = "name_en")]
    pub name_en: String,
    pub symbol: String,
    #[serde(rename = "upright_keywords")]
    pub upright_keywords: Vec<String>,
    #[serde(rename = "reversed_keywords")]
    pub reversed_keywords: Vec<String>,
    #[serde(rename = "imagePath")]
    pub image_path: String,
}

#[derive(Clone, Debug, Deserialize)]
struct RuntimeDeck {
    #[serde(rename = "deckId")]
    deck_id: String,
    cards: Vec<TarotCard>,
}

#[derive(Clone, Debug)]
pub struct TarotDeck {
    pub cards: Vec<TarotCard>,
}

impl TarotDeck {
    pub fn load_bundled() -> Result<Self, String> {
        let runtime: RuntimeDeck = serde_json::from_str(RUNTIME_DATA)
            .map_err(|error| format!("Bundled tarot data is invalid: {error}"))?;
        if runtime.deck_id != TAROT_DATASET_VERSION {
            return Err("Bundled tarot dataset version does not match the app".into());
        }
        Self::from_cards(runtime.cards)
    }

    pub fn from_cards(cards: Vec<TarotCard>) -> Result<Self, String> {
        validate_cards(&cards)?;
        Ok(Self { cards })
    }
}

pub fn validate_cards(cards: &[TarotCard]) -> Result<(), String> {
    if cards.len() != 78 {
        return Err(format!("Expected 78 cards, got {}", cards.len()));
    }
    let mut ids = HashSet::new();
    let mut chinese_names = HashSet::new();
    let mut english_names = HashSet::new();
    let mut image_paths = HashSet::new();
    let mut major_count = 0;
    let mut suits = [0usize; 4];
    for card in cards {
        if !ids.insert(card.id.as_str()) {
            return Err(format!("Duplicate card id: {}", card.id));
        }
        if !chinese_names.insert(card.name_zh.as_str())
            || !english_names.insert(card.name_en.as_str())
        {
            return Err(format!("Duplicate card name: {}", card.id));
        }
        if !image_paths.insert(card.image_path.as_str()) {
            return Err(format!("Duplicate card image path: {}", card.id));
        }
        if !card.image_path.starts_with("/assets/tarot/rws/") || !card.image_path.ends_with(".webp")
        {
            return Err(format!("Card image is not a local RWS WebP: {}", card.id));
        }
        if card.upright_keywords.is_empty()
            || card
                .upright_keywords
                .iter()
                .any(|value| value.trim().is_empty())
        {
            return Err(format!("Missing upright keywords: {}", card.id));
        }
        if card.reversed_keywords.is_empty()
            || card
                .reversed_keywords
                .iter()
                .any(|value| value.trim().is_empty())
        {
            return Err(format!("Missing reversed keywords: {}", card.id));
        }
        if card.symbol.trim().is_empty() {
            return Err(format!("Missing card symbol: {}", card.id));
        }
        match card.arcana.as_str() {
            "major" => {
                major_count += 1;
                if card.number.is_none() || card.suit.is_some() {
                    return Err(format!("Invalid major arcana metadata: {}", card.id));
                }
            }
            "minor" => {
                let index = match card.suit.as_deref() {
                    Some("wands") => 0,
                    Some("cups") => 1,
                    Some("swords") => 2,
                    Some("pentacles") => 3,
                    _ => return Err(format!("Invalid minor suit: {}", card.id)),
                };
                if card.rank.as_deref().unwrap_or_default().is_empty() || card.number.is_some() {
                    return Err(format!("Invalid minor rank metadata: {}", card.id));
                }
                suits[index] += 1;
            }
            _ => return Err(format!("Invalid arcana type: {}", card.id)),
        }
    }
    if major_count != 22 || suits != [14, 14, 14, 14] {
        return Err(format!(
            "Invalid deck composition: major={major_count}, suits={suits:?}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck() -> TarotDeck {
        TarotDeck::load_bundled().expect("valid bundled deck")
    }

    fn mutate_cards(mutator: impl FnOnce(&mut Vec<TarotCard>)) -> Result<(), String> {
        let mut cards = deck().cards;
        mutator(&mut cards);
        validate_cards(&cards)
    }

    #[test]
    fn bundled_runtime_parses() {
        assert!(TarotDeck::load_bundled().is_ok());
    }
    #[test]
    fn bundled_deck_has_78_cards() {
        assert_eq!(deck().cards.len(), 78);
    }
    #[test]
    fn bundled_deck_has_22_majors() {
        assert_eq!(
            deck().cards.iter().filter(|c| c.arcana == "major").count(),
            22
        );
    }
    #[test]
    fn bundled_deck_has_56_minors() {
        assert_eq!(
            deck().cards.iter().filter(|c| c.arcana == "minor").count(),
            56
        );
    }
    #[test]
    fn bundled_wands_has_14_cards() {
        assert_eq!(
            deck()
                .cards
                .iter()
                .filter(|c| c.suit.as_deref() == Some("wands"))
                .count(),
            14
        );
    }
    #[test]
    fn bundled_cups_has_14_cards() {
        assert_eq!(
            deck()
                .cards
                .iter()
                .filter(|c| c.suit.as_deref() == Some("cups"))
                .count(),
            14
        );
    }
    #[test]
    fn bundled_swords_has_14_cards() {
        assert_eq!(
            deck()
                .cards
                .iter()
                .filter(|c| c.suit.as_deref() == Some("swords"))
                .count(),
            14
        );
    }
    #[test]
    fn bundled_pentacles_has_14_cards() {
        assert_eq!(
            deck()
                .cards
                .iter()
                .filter(|c| c.suit.as_deref() == Some("pentacles"))
                .count(),
            14
        );
    }
    #[test]
    fn stable_card_ids_are_unique() {
        assert_eq!(
            deck()
                .cards
                .iter()
                .map(|c| &c.id)
                .collect::<HashSet<_>>()
                .len(),
            78
        );
    }
    #[test]
    fn chinese_names_are_unique() {
        assert_eq!(
            deck()
                .cards
                .iter()
                .map(|c| &c.name_zh)
                .collect::<HashSet<_>>()
                .len(),
            78
        );
    }
    #[test]
    fn english_names_are_unique() {
        assert_eq!(
            deck()
                .cards
                .iter()
                .map(|c| &c.name_en)
                .collect::<HashSet<_>>()
                .len(),
            78
        );
    }
    #[test]
    fn every_card_has_upright_keywords() {
        assert!(deck().cards.iter().all(|c| !c.upright_keywords.is_empty()));
    }
    #[test]
    fn every_card_has_reversed_keywords() {
        assert!(deck().cards.iter().all(|c| !c.reversed_keywords.is_empty()));
    }
    #[test]
    fn every_card_has_a_schema_symbol() {
        assert!(deck().cards.iter().all(|c| !c.symbol.trim().is_empty()));
    }
    #[test]
    fn every_image_path_is_local_webp() {
        assert!(deck()
            .cards
            .iter()
            .all(|c| c.image_path.starts_with("/assets/tarot/rws/")
                && c.image_path.ends_with(".webp")));
    }
    #[test]
    fn image_paths_are_unique() {
        assert_eq!(
            deck()
                .cards
                .iter()
                .map(|c| &c.image_path)
                .collect::<HashSet<_>>()
                .len(),
            78
        );
    }
    #[test]
    fn dataset_version_is_fixed_to_verified_edition() {
        assert_eq!(TAROT_DATASET_VERSION, "rws_pam_a_taionwc_v1");
    }
    #[test]
    fn prototype_demo_score_is_not_in_runtime_contract() {
        let value: serde_json::Value = serde_json::from_str(RUNTIME_DATA).unwrap();
        assert!(value["cards"][0].get("tone_for_demo_only").is_none());
    }
    #[test]
    fn prototype_geometric_art_is_not_used_as_card_face() {
        assert!(deck().cards.iter().all(|c| c.image_path.ends_with(".webp")));
    }
    #[test]
    fn validator_rejects_missing_card() {
        assert!(mutate_cards(|cards| {
            cards.pop();
        })
        .is_err());
    }
    #[test]
    fn validator_rejects_duplicate_id() {
        assert!(mutate_cards(|cards| {
            cards[1].id = cards[0].id.clone();
        })
        .is_err());
    }
    #[test]
    fn validator_rejects_duplicate_chinese_name() {
        assert!(mutate_cards(|cards| {
            cards[1].name_zh = cards[0].name_zh.clone();
        })
        .is_err());
    }
    #[test]
    fn validator_rejects_duplicate_english_name() {
        assert!(mutate_cards(|cards| {
            cards[1].name_en = cards[0].name_en.clone();
        })
        .is_err());
    }
    #[test]
    fn validator_rejects_duplicate_image_path() {
        assert!(mutate_cards(|cards| {
            cards[1].image_path = cards[0].image_path.clone();
        })
        .is_err());
    }
    #[test]
    fn validator_rejects_external_image() {
        assert!(mutate_cards(|cards| {
            cards[0].image_path = "https://example.test/fool.webp".into();
        })
        .is_err());
    }
    #[test]
    fn validator_rejects_unknown_arcana() {
        assert!(mutate_cards(|cards| {
            cards[0].arcana = "custom".into();
        })
        .is_err());
    }
    #[test]
    fn validator_rejects_major_without_number() {
        assert!(mutate_cards(|cards| {
            cards[0].number = None;
        })
        .is_err());
    }
    #[test]
    fn validator_rejects_minor_without_rank() {
        assert!(mutate_cards(|cards| {
            cards[22].rank = None;
        })
        .is_err());
    }
    #[test]
    fn validator_rejects_unknown_minor_suit() {
        assert!(mutate_cards(|cards| {
            cards[22].suit = Some("stars".into());
        })
        .is_err());
    }
    #[test]
    fn validator_rejects_missing_upright_keywords() {
        assert!(mutate_cards(|cards| {
            cards[0].upright_keywords.clear();
        })
        .is_err());
    }
    #[test]
    fn validator_rejects_missing_reversed_keywords() {
        assert!(mutate_cards(|cards| {
            cards[0].reversed_keywords.clear();
        })
        .is_err());
    }
    #[test]
    fn validator_rejects_missing_symbol() {
        assert!(mutate_cards(|cards| {
            cards[0].symbol.clear();
        })
        .is_err());
    }
}
