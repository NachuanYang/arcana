import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createMemoryHistory } from "vue-router";
import { createAppRouter } from "../router";
import { clearAutoReadViewport, takeAutoReadViewport } from "../router/auto-read-viewport";
import { tarotDrawStore } from "../tarot/tarot-draw-flow";
import DrawView from "./DrawView.vue";

const { nativeApi, sessionApi } = vi.hoisted(() => ({
  nativeApi: {
    createDraw: vi.fn(),
    revealDraw: vi.fn(),
    discardDraw: vi.fn(),
  },
  sessionApi: { getSession: vi.fn(), listSessions: vi.fn(), deleteSession: vi.fn() },
}));

vi.mock("../tarot/tauri-tarot-draw-api", () => ({ tauriTarotDrawApi: nativeApi }));
vi.mock("../tarot/tauri-session-api", () => ({ tauriTarotSessionApi: sessionApi }));

function revealSnapshot(drawId: string, question: string) {
  return {
    drawId,
    datasetVersion: "rws_pam_a_taionwc_v1",
    question,
    cards: ["present", "obstacle", "tendency"].map((slot, visualOrder) => ({
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
}

describe("DrawView", () => {
  beforeEach(async () => {
    clearAutoReadViewport();
    Object.assign(tarotDrawStore.state, {
      question: "",
      offer: null,
      selectedPositions: [],
      revealed: null,
      busy: false,
      error: "",
    });
    nativeApi.createDraw.mockImplementation(async (question: string) => ({
      drawId: "opaque-draw-1",
      question,
      backs: Array.from({ length: 12 }, (_, position) => ({ position })),
      backImagePath: "/assets/tarot/back.svg",
    }));
    nativeApi.revealDraw.mockImplementation(async (drawId: string, _positions: number[]) => revealSnapshot(drawId, "项目如何走"));
    nativeApi.discardDraw.mockResolvedValue(true);
    sessionApi.getSession.mockImplementation(async (id: string) => ({
      id,
      question: "项目如何走",
      spreadId: "three_present_obstacle_tendency",
      deckVersion: "rws_pam_a_taionwc_v1",
      state: "REVEALED",
      createdAt: "2026-10-12T10:00:00Z",
      revealedAt: "2026-10-12T10:00:00Z",
      updatedAt: "2026-10-12T10:00:00Z",
      readingStance: null,
      cards: revealSnapshot(id, "项目如何走").cards,
      turns: [],
    }));
    await tarotDrawStore.start("项目如何走");
  });

  it("shows twelve keyboard-operable identical backs without hidden card metadata", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/reading/draw/opaque-draw-1");
    const wrapper = mount(DrawView, { global: { plugins: [router] } });
    const backs = wrapper.findAll("button.card-back");

    expect(backs).toHaveLength(12);
    expect(backs.every((button) => button.attributes("type") === "button")).toBe(true);
    expect(backs.map((button) => button.attributes("aria-label"))).toEqual(
      Array.from({ length: 12 }, (_, position) => `第${position + 1}张牌背，未选`),
    );
    expect(wrapper.get(".card-back-grid").attributes("aria-label")).toBe("十二张可选择的牌背");
    expect(wrapper.html()).not.toMatch(/major_|minor_|The Fool|愚者|upright|reversed/);
    expect(wrapper.findAll("[title]")).toHaveLength(0);
    expect(wrapper.find("button.cancel-draw").exists()).toBe(false);
    expect(wrapper.findAll(".draw-actions button")).toHaveLength(1);
    expect(wrapper.get("button.reveal-action").classes()).toContain("centered-action");
    wrapper.unmount();
  });

  it("shows only the accessible question summary and allows correcting a single selected card", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/reading/draw/opaque-draw-1");
    const wrapper = mount(DrawView, { global: { plugins: [router] } });

    const intro = wrapper.get(".draw-intro");
    const summary = intro.get(".question-summary");
    expect(summary.text()).toBe("项目如何走");
    expect(summary.attributes("aria-label")).toBe("项目如何走");
    expect(intro.findAll("h1")).toHaveLength(0);
    expect(intro.text()).not.toContain("你提出的问题");
    expect(wrapper.find(".undo-selection").exists()).toBe(false);

    const firstCard = wrapper.findAll("button.card-back")[0];
    await firstCard.trigger("click");
    expect(firstCard.classes()).toContain("selected");
    await firstCard.trigger("click");
    expect(firstCard.classes()).not.toContain("selected");
    wrapper.unmount();
  });

  it("reveals only after three unique selections and then renders the frozen spread", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/reading/draw/opaque-draw-1");
    const wrapper = mount(DrawView, { global: { plugins: [router] } });
    const backs = wrapper.findAll("button.card-back");

    await backs[0].trigger("click");
    await backs[4].trigger("click");
    expect((wrapper.get("button.reveal-action").element as HTMLButtonElement).disabled).toBe(true);
    await backs[8].trigger("click");
    await wrapper.get("button.reveal-action").trigger("click");
    await flushPromises();

    expect(nativeApi.revealDraw).toHaveBeenCalledWith("opaque-draw-1", [0, 4, 8]);
    expect(router.currentRoute.value.name).toBe("session-detail");
    expect(router.currentRoute.value.params.sessionId).toBe("opaque-draw-1");
    expect(router.currentRoute.value.query.autoRead).toBe("1");
    expect(tarotDrawStore.state.revealed?.cards).toHaveLength(3);
    wrapper.unmount();
  });

  it("preserves the draw viewport for the automatic reading route", async () => {
    const originalScrollY = Object.getOwnPropertyDescriptor(window, "scrollY");
    const originalScrollHeight = Object.getOwnPropertyDescriptor(document.documentElement, "scrollHeight");
    const originalInnerHeight = Object.getOwnPropertyDescriptor(window, "innerHeight");
    Object.defineProperty(window, "scrollY", { configurable: true, value: 460 });
    Object.defineProperty(document.documentElement, "scrollHeight", { configurable: true, value: 2200 });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 800 });

    const router = createAppRouter(createMemoryHistory());
    await router.push("/reading/draw/opaque-draw-1");
    const wrapper = mount(DrawView, { global: { plugins: [router] } });
    const backs = wrapper.findAll("button.card-back");
    await backs[0].trigger("click");
    await backs[4].trigger("click");
    await backs[8].trigger("click");
    await wrapper.get("button.reveal-action").trigger("click");
    await flushPromises();

    expect(takeAutoReadViewport("opaque-draw-1")).toEqual({ top: 460, wasNearBottom: false });
    wrapper.unmount();
    if (originalScrollY) Object.defineProperty(window, "scrollY", originalScrollY);
    if (originalScrollHeight) Object.defineProperty(document.documentElement, "scrollHeight", originalScrollHeight);
    if (originalInnerHeight) Object.defineProperty(window, "innerHeight", originalInnerHeight);
  });
});
