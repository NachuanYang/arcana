import type { TarotSessionSnapshot, TarotTurn } from "../tarot/tarot-draw-store";

export type FollowupResult = {
  schemaVersion: 1;
  sessionId: string;
  replyToTurnId: string;
  answer: string;
  contextSummary?: string;
  citedCards: Array<{
    slot: "present" | "obstacle" | "tendency";
    cardId: string;
    orientation: "upright" | "reversed";
  }>;
  stanceUnchanged: true;
};

export type FollowupHistoryItem = {
  userTurn: TarotTurn;
  question: string;
  assistantTurn: TarotTurn | null;
  answer: FollowupResult | null;
};

const slots = new Set(["present", "obstacle", "tendency"]);

export function validateFollowupResult(
  value: unknown,
  session: TarotSessionSnapshot,
  userTurn: TarotTurn,
): FollowupResult | null {
  if (!value || typeof value !== "object") return null;
  const response = value as Partial<FollowupResult>;
  if (
    response.schemaVersion !== 1
    || response.sessionId !== session.id
    || response.replyToTurnId !== userTurn.id
    || response.stanceUnchanged !== true
    || !session.readingStance
    || !within(response.answer, 40, 1_000)
    || response.contextSummary !== undefined && !within(response.contextSummary, 40, 1_200)
    || !Array.isArray(response.citedCards)
    || response.citedCards.length < 1
    || response.citedCards.length > 3
    || userTurn.role !== "user"
    || userTurn.sessionId !== session.id
  ) return null;

  const cited = new Set<string>();
  for (const item of response.citedCards) {
    if (
      !item
      || !slots.has(item.slot)
      || !session.cards.some((card) =>
        card.slot === item.slot
        && card.deckCardId === item.cardId
        && card.orientation === item.orientation)
      || cited.has(item.cardId)
    ) return null;
    cited.add(item.cardId);
  }
  return response as FollowupResult;
}

export function followupHistory(session: TarotSessionSnapshot): FollowupHistoryItem[] {
  return session.turns
    .filter((turn) => turn.role === "user")
    .sort((left, right) => left.turnOrder - right.turnOrder)
    .flatMap((userTurn) => {
      let question = "";
      try {
        const value: unknown = JSON.parse(userTurn.contentJson);
        if (value && typeof value === "object" && "question" in value && typeof value.question === "string") {
          question = value.question;
        }
      } catch {
        return [];
      }
      const assistantTurn = session.turns.find((turn) =>
        turn.role === "assistant"
        && turn.requestId === userTurn.requestId
        && turn.turnOrder > userTurn.turnOrder);
      let answer: FollowupResult | null = null;
      if (assistantTurn?.status === "COMPLETE") {
        try {
          answer = validateFollowupResult(JSON.parse(assistantTurn.contentJson), session, userTurn);
        } catch {
          answer = null;
        }
      }
      return [{ userTurn, question, assistantTurn: assistantTurn ?? null, answer }];
    });
}

function within(value: unknown, minimum: number, maximum: number): value is string {
  if (typeof value !== "string") return false;
  const length = Array.from(value.trim()).length;
  return length >= minimum && length <= maximum && !value.includes("\0");
}
