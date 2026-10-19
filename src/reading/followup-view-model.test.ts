import { describe, expect, it } from "vitest";
import type { TarotSessionSnapshot, TarotTurn } from "../tarot/tarot-draw-store";
import { followupHistory, validateFollowupResult } from "./followup-view-model";

function session(): TarotSessionSnapshot {
  return {
    id: "session-1", question: "原问题如何推进", spreadId: "three_present_obstacle_tendency",
    deckVersion: "rws_pam_a_taionwc_v1", state: "READING_READY", createdAt: "", revealedAt: "", updatedAt: "",
    readingStance: "CAUTIOUS",
    cards: ["present", "obstacle", "tendency"].map((slot, index) => ({
      slot: slot as "present" | "obstacle" | "tendency", deckCardId: `card-${index}`, orientation: index === 1 ? "reversed" as const : "upright" as const,
      visualOrder: index, card: { id: `card-${index}`, arcana: "major", name_zh: `牌${index}`, name_en: `Card ${index}`, symbol: "✦", upright_keywords: ["清晰"], reversed_keywords: ["迟疑"], imagePath: "/card.webp" },
    })),
    turns: [],
  };
}

function userTurn(): TarotTurn {
  return { id: "user-turn", sessionId: "session-1", turnOrder: 2, role: "user", requestId: "followup-1", contentJson: JSON.stringify({ question: "为什么建议我谨慎？" }), status: "COMPLETE", modelId: null, createdAt: "" };
}

function result() {
  return {
    schemaVersion: 1 as const, sessionId: "session-1", replyToTurnId: "user-turn",
    answer: "这份谨慎来自阻碍位的逆位提示和趋向位仍需观察的信号，并不否定你的推进意愿。先确认资源条件，再用小步骤收集现实反馈。",
    citedCards: [{ slot: "obstacle" as const, cardId: "card-1", orientation: "reversed" as const }],
    stanceUnchanged: true as const,
  };
}

describe("validated same-spread follow-ups", () => {
  it("accepts only a same-session reply to the exact user turn and frozen citation", () => {
    const saved = session();
    expect(validateFollowupResult(result(), saved, userTurn())?.stanceUnchanged).toBe(true);
  });

  it("rejects changed stance, cross-session answers, and unseen cards", () => {
    const saved = session();
    expect(validateFollowupResult({ ...result(), stanceUnchanged: false }, saved, userTurn())).toBeNull();
    expect(validateFollowupResult({ ...result(), sessionId: "other" }, saved, userTurn())).toBeNull();
    expect(validateFollowupResult({ ...result(), citedCards: [{ ...result().citedCards[0], cardId: "card-9" }] }, saved, userTurn())).toBeNull();
  });

  it("accepts legacy turns without a summary but rejects an invalid stored summary", () => {
    const saved = session();
    expect(validateFollowupResult(result(), saved, userTurn())).not.toBeNull();
    expect(validateFollowupResult({ ...result(), contextSummary: "too short" }, saved, userTurn())).toBeNull();
    expect(validateFollowupResult({ ...result(), contextSummary: "摘要".repeat(601) }, saved, userTurn())).toBeNull();
  });

  it("restores answered and failed follow-ups from local turn snapshots", () => {
    const saved = session();
    const user = userTurn();
    saved.turns = [
      user,
      { id: "assistant-turn", sessionId: saved.id, turnOrder: 3, role: "assistant", requestId: user.requestId, contentJson: JSON.stringify(result()), status: "COMPLETE", modelId: "gpt-test", createdAt: "" },
    ];
    expect(followupHistory(saved)[0].answer?.answer).toContain("推进意愿");
    saved.turns[1].status = "FAILED_RETRYABLE";
    expect(followupHistory(saved)[0].answer).toBeNull();
    expect(followupHistory(saved)[0].question).toBe("为什么建议我谨慎？");
  });
});
