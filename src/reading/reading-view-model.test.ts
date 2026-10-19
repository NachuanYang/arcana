import { describe, expect, it } from "vitest";
import type { TarotSessionSnapshot } from "../tarot/tarot-draw-store";
import { latestCompleteReading, validateReadingResult } from "./reading-view-model";

function makeSession(): TarotSessionSnapshot {
  return {
    id: "session-1", question: "目前的项目方向如何", spreadId: "three_present_obstacle_tendency",
    deckVersion: "rws_pam_a_taionwc_v1", state: "READING_READY", createdAt: "", revealedAt: "", updatedAt: "",
    readingStance: "CAUTIOUS",
    cards: ["present", "obstacle", "tendency"].map((slot, index) => ({
      slot: slot as "present" | "obstacle" | "tendency", deckCardId: `card-${index}`, orientation: index === 1 ? "reversed" as const : "upright" as const,
      visualOrder: index, card: { id: `card-${index}`, arcana: "major", name_zh: `牌${index}`, name_en: `Card ${index}`, symbol: "✦", upright_keywords: ["清晰"], reversed_keywords: ["迟疑"], imagePath: "/card.webp" },
    })),
    turns: [],
  };
}

function makeResult() {
  return {
    schemaVersion: 1, sessionId: "session-1", stance: "CAUTIOUS",
    headline: "方向可行，但需要先处理现实阻力",
    synthesis: "现状显示你已经具备启动条件，但阻碍位提示资源和沟通仍有缺口。趋向并非自动成功；若先补齐关键条件，推进空间会更清楚。".repeat(3),
    cards: ["present", "obstacle", "tendency"].map((slot, index) => ({
      slot, cardId: `card-${index}`, orientation: index === 1 ? "reversed" : "upright",
      explanation: `这张牌在${slot}位置提示需要观察现实条件，并结合牌面关键词谨慎判断，不把象征倾向当成确定事实。`,
    })),
    tension: "你有推进动力，眼下资源准备却还不充分。",
    actionableReflection: "先确认最关键的一项资源是否到位，再决定下一步节奏。",
    limitsNote: "这是一种象征性反思，不是对现实结果的证明。",
  };
}

describe("validated reading presentation", () => {
  it("accepts a sectioned result only when it matches the saved spread", () => {
    expect(validateReadingResult(makeResult(), makeSession())?.cards).toHaveLength(3);
  });

  it("rejects altered cards, wrong sessions, and malformed persisted JSON", () => {
    const result = makeResult();
    result.cards[1].cardId = "unseen-card";
    expect(validateReadingResult(result, makeSession())).toBeNull();
    expect(validateReadingResult({ ...makeResult(), sessionId: "other" }, makeSession())).toBeNull();
    const session = makeSession();
    session.turns = [{ id: "t", sessionId: session.id, turnOrder: 1, role: "assistant", requestId: "r", contentJson: "{", status: "COMPLETE", modelId: "m", createdAt: "" }];
    expect(latestCompleteReading(session)).toBeNull();
  });

  it("restores only a validated complete assistant turn", () => {
    const session = makeSession();
    session.turns = [{ id: "t", sessionId: session.id, turnOrder: 1, role: "assistant", requestId: "r", contentJson: JSON.stringify(makeResult()), status: "COMPLETE", modelId: "m", createdAt: "" }];
    expect(latestCompleteReading(session)?.stance).toBe("CAUTIOUS");
    session.turns[0].status = "STREAMING";
    expect(latestCompleteReading(session)).toBeNull();
  });

  it("keeps the initial reading available after follow-up assistant turns are appended", () => {
    const session = makeSession();
    const initial = makeResult();
    session.turns = [
      { id: "initial", sessionId: session.id, turnOrder: 1, role: "assistant", requestId: "initial-request", contentJson: JSON.stringify(initial), status: "COMPLETE", modelId: "m", createdAt: "" },
      { id: "followup-user", sessionId: session.id, turnOrder: 2, role: "user", requestId: "followup-request", contentJson: JSON.stringify({ question: "我应该先核实什么？" }), status: "COMPLETE", modelId: null, createdAt: "" },
      { id: "followup-answer", sessionId: session.id, turnOrder: 3, role: "assistant", requestId: "followup-request", contentJson: JSON.stringify({
        schemaVersion: 1,
        sessionId: session.id,
        replyToTurnId: "followup-user",
        answer: "先核对手上的资源与截止时间，再决定是否推进。",
        contextSummary: "用户正在谨慎评估项目风险，仍需核实资源、沟通进度和截止时间，再安排下一步行动。",
        citedCards: [{ slot: "obstacle", cardId: "card-1", orientation: "reversed" }],
        stanceUnchanged: true,
      }), status: "COMPLETE", modelId: "m", createdAt: "" },
    ];

    expect(latestCompleteReading(session)?.headline).toBe(initial.headline);
  });
});
