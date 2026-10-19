import { describe, expect, it, vi } from "vitest";
import { createTarotCatalogStore, type TarotCatalogCard } from "./tarot-catalog";

function card(id: string): TarotCatalogCard {
  return {
    id,
    arcana: "major",
    number: 0,
    name_zh: "愚者",
    name_en: "The Fool",
    symbol: "✧",
    upright_keywords: ["启程"],
    reversed_keywords: ["鲁莽"],
    imagePath: `/assets/tarot/rws/${id}.webp`,
  };
}

function completeDeck(): TarotCatalogCard[] {
  return Array.from({ length: 78 }, (_, index) => card(index === 0 ? "major_00" : `test_${index}`));
}

describe("offline tarot catalog", () => {
  it("loads verified card metadata from the local API", async () => {
    const store = createTarotCatalogStore({ listCards: vi.fn().mockResolvedValue(completeDeck()) });

    await store.load();

    expect(store.state.cards).toEqual(completeDeck());
    expect(store.state.loaded).toBe(true);
    expect(store.state.error).toBe("");
  });

  it("rejects an incomplete directory instead of implying the full deck is available", async () => {
    const store = createTarotCatalogStore({ listCards: vi.fn().mockResolvedValue([card("major_00")]) });

    await store.load();

    expect(store.state.loaded).toBe(false);
    expect(store.state.error).toContain("1/78");
  });

  it("shares an in-flight load instead of duplicating the native request", async () => {
    let finish!: (cards: TarotCatalogCard[]) => void;
    const listCards = vi.fn(() => new Promise<TarotCatalogCard[]>((resolve) => { finish = resolve; }));
    const store = createTarotCatalogStore({ listCards });

    const first = store.load();
    const second = store.load();
    await Promise.resolve();
    finish(completeDeck());
    await Promise.all([first, second]);

    expect(listCards).toHaveBeenCalledTimes(1);
  });

  it("keeps a visible error and allows retry after failure", async () => {
    const listCards = vi.fn().mockRejectedValueOnce(new Error("local deck unavailable")).mockResolvedValueOnce(completeDeck());
    const store = createTarotCatalogStore({ listCards });

    await store.load();
    expect(store.state.error).toContain("local deck unavailable");
    expect(store.state.loaded).toBe(false);
    await store.load();

    expect(store.state.loaded).toBe(true);
    expect(store.state.cards).toHaveLength(78);
    expect(listCards).toHaveBeenCalledTimes(2);
  });
});
