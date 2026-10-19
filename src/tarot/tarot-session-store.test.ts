import { describe, expect, it, vi } from "vitest";
import { createTarotSessionStore } from "./tarot-session-store";
import type { TarotSessionApi } from "./tauri-session-api";
import type { DrawnCard, TarotSessionSnapshot, TarotSessionSummary } from "./tarot-draw-store";

const cards: DrawnCard[] = (["present", "obstacle", "tendency"] as const).map((slot, visualOrder) => ({
  slot,
  deckCardId: `major_${visualOrder}`,
  orientation: "upright",
  visualOrder,
  card: {
    id: `major_${visualOrder}`,
    arcana: "major",
    number: visualOrder,
    name_zh: `牌${visualOrder}`,
    name_en: `Card ${visualOrder}`,
    symbol: "✧",
    upright_keywords: ["关键词"],
    reversed_keywords: ["逆位关键词"],
    imagePath: `/assets/tarot/rws/major_${visualOrder}.webp`,
  },
}));

function session(id: string, createdAt: string): TarotSessionSnapshot {
  return {
    id,
    question: `问题 ${id}`,
    spreadId: "three_present_obstacle_tendency",
    deckVersion: "rws_pam_a_taionwc_v1",
    state: "REVEALED",
    createdAt,
    revealedAt: createdAt,
    updatedAt: createdAt,
    readingStance: null,
    cards,
    turns: [],
  };
}

function summary(value: TarotSessionSnapshot): TarotSessionSummary {
  return {
    id: value.id,
    question: value.question,
    state: value.state,
    createdAt: value.createdAt,
    revealedAt: value.revealedAt,
    cards: value.cards,
    turnCount: value.turns.length,
  };
}

function api(sessions: TarotSessionSnapshot[]): TarotSessionApi {
  return {
    getSession: vi.fn(async (id: string) => sessions.find((item) => item.id === id) ?? null),
    listSessions: vi.fn(async () => sessions.map(summary)),
    deleteSession: vi.fn(async (id: string) => {
      const index = sessions.findIndex((item) => item.id === id);
      if (index < 0) return false;
      sessions.splice(index, 1);
      return true;
    }),
  };
}

describe("persistent tarot session store", () => {
  it("loads history and restores a saved session after a renderer restart", async () => {
    const saved = session("saved-session", "2026-10-12T10:00:00Z");
    const backend = api([saved]);
    const store = createTarotSessionStore(backend);

    await store.refresh();
    await store.open(saved.id);

    expect(store.state.sessions.map((item) => item.id)).toEqual([saved.id]);
    expect(store.state.current).toEqual(saved);
    expect(backend.getSession).toHaveBeenCalledWith(saved.id);
  });

  it("deletes a session from the refreshed history only after the API succeeds", async () => {
    const saved = session("saved-session", "2026-10-12T10:00:00Z");
    const backend = api([saved]);
    const store = createTarotSessionStore(backend);
    await store.refresh();
    await store.open(saved.id);

    await store.delete(saved.id);

    expect(backend.deleteSession).toHaveBeenCalledWith(saved.id);
    expect(store.state.sessions).toEqual([]);
    expect(store.state.current).toBeNull();
  });

  it("keeps loaded history visible when a refresh fails", async () => {
    const saved = session("saved-session", "2026-10-12T10:00:00Z");
    const backend = api([saved]);
    const store = createTarotSessionStore(backend);
    await store.refresh();
    vi.mocked(backend.listSessions).mockRejectedValueOnce(new Error("offline"));

    await store.refresh();

    expect(store.state.sessions).toHaveLength(1);
    expect(store.state.error).toContain("offline");
  });

  it("rejects incomplete or mismatched snapshots instead of displaying them", async () => {
    const saved = session("saved-session", "2026-10-12T10:00:00Z");
    saved.cards[1].slot = "present";
    const backend = api([saved]);
    const store = createTarotSessionStore(backend);

    await store.open(saved.id);

    expect(store.state.current).toBeNull();
    expect(store.state.error).toContain("快照无效");
  });
});
