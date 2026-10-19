import { describe, expect, it, vi } from "vitest";
import { createTarotDrawStore, effectiveQuestionLength, type DrawOffer, type RevealSnapshot } from "./tarot-draw-store";

const question = "项目如何走";
const drawOffer: DrawOffer = {
  drawId: "opaque-draw-1",
  question,
  backs: Array.from({ length: 12 }, (_, position) => ({ position })),
  backImagePath: "/assets/tarot/back.svg",
};
const snapshot: RevealSnapshot = {
  drawId: drawOffer.drawId,
  datasetVersion: "rws_pam_a_taionwc_v1",
  question,
  cards: (["present", "obstacle", "tendency"] as const).map((slot, visualOrder) => ({
    slot,
    deckCardId: `major_0${visualOrder}`,
    orientation: visualOrder === 1 ? "reversed" : "upright",
    visualOrder,
    card: {
      id: `major_0${visualOrder}`,
      arcana: "major",
      number: visualOrder,
      name_zh: ["愚者", "魔术师", "女祭司"][visualOrder],
      name_en: ["The Fool", "The Magician", "The High Priestess"][visualOrder],
      symbol: "✧",
      upright_keywords: ["启程"],
      reversed_keywords: ["鲁莽"],
      imagePath: `/assets/tarot/rws/major_0${visualOrder}.webp`,
    },
  })),
};

function api() {
  return {
    createDraw: vi.fn().mockResolvedValue(drawOffer),
    revealDraw: vi.fn().mockResolvedValue(snapshot),
    discardDraw: vi.fn().mockResolvedValue(true),
  };
}

describe("question-to-draw store", () => {
  it("counts Unicode question characters while excluding whitespace", () => {
    expect(effectiveQuestionLength(" 项 目 如\n何 走 ")).toBe(5);
  });

  it("retains invalid question input and does not create a draw", async () => {
    const backend = api();
    const store = createTarotDrawStore(backend);

    await store.start("项目如何");

    expect(store.state.question).toBe("项目如何");
    expect(store.state.error).toContain("5");
    expect(backend.createDraw).not.toHaveBeenCalled();
  });

  it("rejects more than 220 effective characters without truncating input", async () => {
    const backend = api();
    const store = createTarotDrawStore(backend);
    const tooLong = "问".repeat(221);

    await store.start(tooLong);

    expect(store.state.question).toBe(tooLong);
    expect(store.state.error).toContain("220");
    expect(backend.createDraw).not.toHaveBeenCalled();
  });

  it("normalizes valid question input and stores only an opaque twelve-back offer", async () => {
    const backend = api();
    const store = createTarotDrawStore(backend);

    await store.start("  项目如何走  ");

    expect(backend.createDraw).toHaveBeenCalledWith(question);
    expect(store.state.question).toBe(question);
    expect(store.state.offer?.backs).toHaveLength(12);
    expect(store.state.revealed).toBeNull();
    expect(JSON.stringify(store.state.offer)).not.toMatch(/major_|minor_|upright|reversed/);
  });

  it("limits selection to three positions and lets each selected position be toggled off", async () => {
    const store = createTarotDrawStore(api());
    await store.start(question);
    store.togglePosition(0);
    store.togglePosition(1);
    store.togglePosition(2);
    store.togglePosition(3);
    expect(store.state.selectedPositions).toEqual([0, 1, 2]);
    store.togglePosition(1);
    expect(store.state.selectedPositions).toEqual([0, 2]);
  });

  it("allows selecting the twelfth back and ignores positions outside the twelve-card pool", async () => {
    const store = createTarotDrawStore(api());
    await store.start(question);

    store.togglePosition(9);
    store.togglePosition(10);
    store.togglePosition(11);
    store.togglePosition(12);

    expect(store.state.selectedPositions).toEqual([9, 10, 11]);
  });

  it("does not request reveal until exactly three backs are selected", async () => {
    const backend = api();
    const store = createTarotDrawStore(backend);
    await store.start(question);
    store.togglePosition(0);
    store.togglePosition(1);

    expect(store.canReveal.value).toBe(false);
    expect(await store.reveal()).toBeNull();
    expect(backend.revealDraw).not.toHaveBeenCalled();
  });

  it("freezes the existing question, selected positions, cards and orientations", async () => {
    const backend = api();
    const store = createTarotDrawStore(backend);
    await store.start(question);
    store.togglePosition(6);
    store.togglePosition(2);
    store.togglePosition(8);

    await store.reveal();

    expect(backend.revealDraw).toHaveBeenCalledWith(drawOffer.drawId, [6, 2, 8]);
    expect(store.state.revealed).toEqual(snapshot);
    expect(store.state.revealed?.question).toBe(question);
    expect(store.state.selectedPositions).toEqual([6, 2, 8]);
    expect(store.canReveal.value).toBe(false);
    await store.reveal();
    expect(backend.revealDraw).toHaveBeenCalledTimes(1);
  });

  it("keeps the chosen backs available after reveal failure", async () => {
    const backend = api();
    backend.revealDraw.mockRejectedValueOnce(new Error("offline"));
    const store = createTarotDrawStore(backend);
    await store.start(question);
    store.togglePosition(0);
    store.togglePosition(3);
    store.togglePosition(7);

    await store.reveal();

    expect(store.state.selectedPositions).toEqual([0, 3, 7]);
    expect(store.state.revealed).toBeNull();
    expect(store.state.error).toContain("offline");
  });

  it("discards the pending draw when cancelling back to the question", async () => {
    const backend = api();
    const store = createTarotDrawStore(backend);
    await store.start(question);
    store.togglePosition(4);

    await store.cancel();

    expect(backend.discardDraw).toHaveBeenCalledWith(drawOffer.drawId);
    expect(store.state.offer).toBeNull();
    expect(store.state.selectedPositions).toEqual([]);
    expect(store.state.question).toBe(question);
  });

  it("blocks duplicate start submissions while the native call is pending", async () => {
    let finish!: (offer: DrawOffer) => void;
    const backend = api();
    backend.createDraw.mockImplementation(() => new Promise<DrawOffer>((resolve) => { finish = resolve; }));
    const store = createTarotDrawStore(backend);

    const first = store.start(question);
    const second = store.start(question);
    expect(backend.createDraw).toHaveBeenCalledTimes(1);
    finish(drawOffer);
    await Promise.all([first, second]);
  });
});
