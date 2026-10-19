use crate::persistence::TarotSessionSnapshot;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const MIN_HEADLINE_CHARS: usize = 12;
const MAX_HEADLINE_CHARS: usize = 90;
const MIN_SYNTHESIS_CHARS: usize = 120;
const MAX_SYNTHESIS_CHARS: usize = 600;
const MIN_CARD_EXPLANATION_CHARS: usize = 50;
const MAX_CARD_EXPLANATION_CHARS: usize = 160;
const MAX_SUPPORTING_TEXT_CHARS: usize = 320;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadingValidationError {
    MalformedSchema,
    SessionMismatch,
    CardCountInvalid,
    TextInvalid,
    FrozenCardMismatch,
}

impl ReadingValidationError {
    pub fn code(self) -> &'static str {
        match self {
            Self::MalformedSchema => "invalid_output_schema",
            Self::SessionMismatch => "reading_session_mismatch",
            Self::CardCountInvalid => "reading_card_count_invalid",
            Self::TextInvalid => "reading_text_invalid",
            Self::FrozenCardMismatch => "frozen_card_mismatch",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReadingStance {
    Favorable,
    Cautious,
    Unfavorable,
    Indeterminate,
}

impl ReadingStance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Favorable => "FAVORABLE",
            Self::Cautious => "CAUTIOUS",
            Self::Unfavorable => "UNFAVORABLE",
            Self::Indeterminate => "INDETERMINATE",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpreadSlot {
    Present,
    Obstacle,
    Tendency,
}

impl SpreadSlot {
    fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Obstacle => "obstacle",
            Self::Tendency => "tendency",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CardOrientation {
    Upright,
    Reversed,
}

impl CardOrientation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Upright => "upright",
            Self::Reversed => "reversed",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CardExplanation {
    pub slot: SpreadSlot,
    pub card_id: String,
    pub orientation: CardOrientation,
    pub explanation: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadingResponse {
    pub schema_version: u8,
    pub session_id: String,
    pub stance: ReadingStance,
    pub headline: String,
    pub synthesis: String,
    pub cards: Vec<CardExplanation>,
    pub tension: String,
    pub actionable_reflection: String,
    pub limits_note: String,
}

pub fn json_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "schemaVersion":{"type":"integer"},
            "sessionId":{"type":"string"},
            "stance":{"type":"string","enum":["FAVORABLE","CAUTIOUS","UNFAVORABLE","INDETERMINATE"]},
            "headline":{"type":"string"},
            "synthesis":{"type":"string"},
            "cards":{"type":"array","items":{
                "type":"object",
                "properties":{
                    "slot":{"type":"string","enum":["present","obstacle","tendency"]},
                    "cardId":{"type":"string"},
                    "orientation":{"type":"string","enum":["upright","reversed"]},
                    "explanation":{"type":"string"}
                },
                "required":["slot","cardId","orientation","explanation"],
                "additionalProperties":false
            }},
            "tension":{"type":"string"},
            "actionableReflection":{"type":"string"},
            "limitsNote":{"type":"string"}
        },
        "required":[
            "schemaVersion","sessionId","stance","headline","synthesis","cards",
            "tension","actionableReflection","limitsNote"
        ],
        "additionalProperties":false
    })
}

pub fn parse_and_validate_reading(
    raw_json: &str,
    session: &TarotSessionSnapshot,
) -> Result<ReadingResponse, ReadingValidationError> {
    let response: ReadingResponse =
        serde_json::from_str(raw_json).map_err(|_| ReadingValidationError::MalformedSchema)?;
    if response.schema_version != 1 {
        return Err(ReadingValidationError::MalformedSchema);
    }
    if response.session_id != session.id {
        return Err(ReadingValidationError::SessionMismatch);
    }
    if response.cards.len() != 3 || session.cards.len() != 3 {
        return Err(ReadingValidationError::CardCountInvalid);
    }
    validate_text(
        &response.headline,
        MIN_HEADLINE_CHARS,
        MAX_HEADLINE_CHARS,
        "headline",
    )?;
    validate_text(
        &response.synthesis,
        MIN_SYNTHESIS_CHARS,
        MAX_SYNTHESIS_CHARS,
        "synthesis",
    )?;
    validate_text(&response.tension, 10, MAX_SUPPORTING_TEXT_CHARS, "tension")?;
    validate_text(
        &response.actionable_reflection,
        10,
        MAX_SUPPORTING_TEXT_CHARS,
        "actionable_reflection",
    )?;
    validate_text(
        &response.limits_note,
        10,
        MAX_SUPPORTING_TEXT_CHARS,
        "limits_note",
    )?;

    for (generated, frozen) in response.cards.iter().zip(&session.cards) {
        if generated.slot.as_str() != frozen.slot
            || generated.card_id != frozen.deck_card_id
            || generated.orientation.as_str() != frozen.orientation
        {
            return Err(ReadingValidationError::FrozenCardMismatch);
        }
        validate_text(
            &generated.explanation,
            MIN_CARD_EXPLANATION_CHARS,
            MAX_CARD_EXPLANATION_CHARS,
            "card explanation",
        )?;
    }
    Ok(response)
}

fn validate_text(
    value: &str,
    minimum: usize,
    maximum: usize,
    _name: &str,
) -> Result<(), ReadingValidationError> {
    let trimmed = value.trim();
    let length = trimmed.chars().count();
    if trimmed.is_empty() || length < minimum || length > maximum || trimmed.contains('\0') {
        return Err(ReadingValidationError::TextInvalid);
    }
    Ok(())
}
