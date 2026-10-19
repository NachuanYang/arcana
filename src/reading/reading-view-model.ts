import type { TarotSessionSnapshot } from "../tarot/tarot-draw-store";

export type ReadingStance = "FAVORABLE" | "CAUTIOUS" | "UNFAVORABLE" | "INDETERMINATE";
export type ReadingResult = {
  schemaVersion: 1;
  sessionId: string;
  stance: ReadingStance;
  headline: string;
  synthesis: string;
  cards: Array<{
    slot: "present" | "obstacle" | "tendency";
    cardId: string;
    orientation: "upright" | "reversed";
    explanation: string;
  }>;
  tension: string;
  actionableReflection: string;
  limitsNote: string;
};

const stances = new Set<ReadingStance>(["FAVORABLE", "CAUTIOUS", "UNFAVORABLE", "INDETERMINATE"]);
const slots = ["present", "obstacle", "tendency"] as const;

export function validateReadingResult(value: unknown, session: TarotSessionSnapshot): ReadingResult | null {
  if (!value || typeof value !== "object") return null;
  const candidate = value as Partial<ReadingResult>;
  if (
    candidate.schemaVersion !== 1
    || candidate.sessionId !== session.id
    || !stances.has(candidate.stance as ReadingStance)
    || !within(candidate.headline, 12, 90)
    || !within(candidate.synthesis, 120, 600)
    || !within(candidate.tension, 10, 320)
    || !within(candidate.actionableReflection, 10, 320)
    || !within(candidate.limitsNote, 10, 320)
    || !Array.isArray(candidate.cards)
    || candidate.cards.length !== 3
    || session.cards.length !== 3
  ) return null;

  for (let index = 0; index < slots.length; index += 1) {
    const generated = candidate.cards[index];
    const frozen = session.cards[index];
    if (
      !generated
      || generated.slot !== slots[index]
      || generated.cardId !== frozen.deckCardId
      || generated.orientation !== frozen.orientation
      || !within(generated.explanation, 50, 160)
    ) return null;
  }
  return candidate as ReadingResult;
}

export function latestCompleteReading(session: TarotSessionSnapshot): ReadingResult | null {
  for (const turn of [...session.turns].reverse()) {
    if (turn.role !== "assistant" || turn.status !== "COMPLETE") continue;
    try {
      const reading = validateReadingResult(JSON.parse(turn.contentJson), session);
      if (reading) return reading;
    } catch {
      // Later follow-up answers are assistant turns too, but are not initial readings.
    }
  }
  return null;
}

function within(value: unknown, minimum: number, maximum: number): value is string {
  if (typeof value !== "string") return false;
  const normalized = value.trim();
  const count = Array.from(normalized).length;
  return count >= minimum && count <= maximum && !normalized.includes("\0");
}
