import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createMemoryHistory } from "vue-router";
import App from "../App.vue";
import { createAppRouter } from "../router";
import { tarotDrawStore } from "../tarot/tarot-draw-flow";
import { tarotSessionStore } from "../tarot/tarot-session-flow";

const { nativeApi, authFlow, startReading, startFollowup } = vi.hoisted(() => ({
  nativeApi: {
    sessions: [] as Array<Record<string, unknown>>,
    createDraw: vi.fn(), revealDraw: vi.fn(), discardDraw: vi.fn(),
    getSession: vi.fn(), listSessions: vi.fn(), deleteSession: vi.fn(),
  },
  authFlow: {
    state: {
      status: { connected: false, planUsageAuthorized: false },
      models: [], selectedModel: "", proxyMode: "system", busy: false, error: "", message: "",
    },
    acceptStatus: vi.fn(), setProxyMode: vi.fn(), refreshStatus: vi.fn().mockResolvedValue(undefined),
    refreshModels: vi.fn().mockResolvedValue(undefined),
  },
  startReading: vi.fn(), startFollowup: vi.fn(),
}));

vi.mock("../tarot/tauri-tarot-draw-api", () => ({ tauriTarotDrawApi: nativeApi }));
vi.mock("../tarot/tauri-session-api", () => ({ tauriTarotSessionApi: nativeApi }));
vi.mock("../auth/auth-flow", () => ({ authFlow, initializeAuthFlow: vi.fn().mockResolvedValue(undefined) }));
vi.mock("../reading/reading-api", () => ({
  readingApi: {
    start: startReading,
    cancel: vi.fn().mockResolvedValue(undefined),
    startFollowup,
    cancelFollowup: vi.fn().mockResolvedValue(undefined),
    listenProgress: vi.fn().mockResolvedValue(() => undefined),
  },
}));

function frozenCards() {
  return [
    { slot: "present", deckCardId: "major_00", orientation: "upright", visualOrder: 0, name_zh: "愚者", name_en: "The Fool", keywords: ["启程"] },
    { slot: "obstacle", deckCardId: "minor_cups_01", orientation: "reversed", visualOrder: 1, name_zh: "圣杯一", name_en: "Ace of Cups", keywords: ["情绪受阻"] },
    { slot: "tendency", deckCardId: "major_06", orientation: "upright", visualOrder: 2, name_zh: "恋人", name_en: "The Lovers", keywords: ["选择"] },
  ].map((frozen) => ({
    ...frozen,
    card: {
      id: frozen.deckCardId,
      arcana: frozen.deckCardId.startsWith("major") ? "major" : "minor",
      number: frozen.visualOrder,
      suit: frozen.deckCardId.startsWith("minor") ? "cups" : undefined,
      rank: undefined,
      name_zh: frozen.name_zh,
      name_en: frozen.name_en,
      symbol: "✧",
      upright_keywords: ["启程"],
      reversed_keywords: ["情绪受阻"],
      imagePath: `/assets/tarot/rws/${frozen.deckCardId}.webp`,
    },
  }));
}

function storedSession(id: string, question: string) {
  const cards = frozenCards();
  return {
    id,
    question,
    spreadId: "three_present_obstacle_tendency",
    deckVersion: "rws_pam_a_taionwc_v1",
    state: "REVEALED",
    createdAt: "2026-10-12T10:00:00.000000000Z",
    revealedAt: "2026-10-12T10:00:00.000000000Z",
    updatedAt: "2026-10-12T10:00:00.000000000Z",
    readingStance: null,
    cards,
    turns: [],
  };
}

describe("offline app journey", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    nativeApi.sessions = [];
    Object.assign(tarotDrawStore.state, { question: "", offer: null, selectedPositions: [], revealed: null, busy: false, error: "" });
    Object.assign(tarotSessionStore.state, { sessions: [], current: null, busy: false, error: "" });
    Object.assign(authFlow.state, {
      status: { connected: false, planUsageAuthorized: false }, models: [], selectedModel: "", proxyMode: "system", busy: false, error: "", message: "",
    });
    nativeApi.createDraw.mockImplementation(async (question: string) => ({
      drawId: "journey-session", question,
      backs: Array.from({ length: 12 }, (_, position) => ({ position })),
      backImagePath: "/assets/tarot/back.svg",
    }));
    nativeApi.revealDraw.mockImplementation(async (id: string, _positions: number[], question: string) => {
      const session = storedSession(id, question || "今天的选择");
      nativeApi.sessions = [session];
      return { drawId: id, datasetVersion: session.deckVersion, question: session.question, cards: session.cards };
    });
    nativeApi.discardDraw.mockResolvedValue(true);
    nativeApi.getSession.mockImplementation(async (id: string) => nativeApi.sessions.find((session) => session.id === id) ?? null);
    nativeApi.listSessions.mockImplementation(async () => nativeApi.sessions.map((session) => ({
      id: session.id, question: session.question, state: session.state,
      createdAt: session.createdAt, revealedAt: session.revealedAt, cards: session.cards, turnCount: 0,
    })));
    nativeApi.deleteSession.mockImplementation(async (id: string) => {
      nativeApi.sessions = nativeApi.sessions.filter((session) => session.id !== id);
      return true;
    });
    startReading.mockResolvedValue(undefined);
    startFollowup.mockResolvedValue(undefined);
  });

  it("completes question, three-card reveal, history review, and confirmed deletion without an account or fabricated answer", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/");
    const wrapper = mount(App, { global: { plugins: [router] } });
    await flushPromises();

    await wrapper.get("textarea[aria-label='占卜问题']").setValue("今天的选择");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(router.currentRoute.value.name).toBe("draw-view");
    expect(nativeApi.createDraw).toHaveBeenCalledWith("今天的选择");

    const backs = wrapper.findAll("button.card-back");
    await backs[0].trigger("click");
    await backs[4].trigger("click");
    await backs[8].trigger("click");
    await wrapper.get("button.reveal-action").trigger("click");
    await flushPromises();
    expect(router.currentRoute.value.name).toBe("session-detail");
    expect(nativeApi.revealDraw).toHaveBeenCalledWith("journey-session", [0, 4, 8]);
    expect(wrapper.findAll("button.revealed-art-frame")).toHaveLength(3);
    expect(wrapper.text()).toContain("请先在设置中连接ChatGPT并完成计划用量授权");
    expect(wrapper.find("a[href='/settings/connection']").exists()).toBe(true);
    expect(wrapper.find(".reading-result-headline").exists()).toBe(false);
    expect(startReading).not.toHaveBeenCalled();
    expect(startFollowup).not.toHaveBeenCalled();

    await wrapper.get("a[href='/history']").trigger("click");
    await flushPromises();
    expect(router.currentRoute.value.name).toBe("history");
    expect(wrapper.text()).toContain("今天的选择");
    expect(wrapper.findAll(".history-card-thumb")).toHaveLength(3);

    await wrapper.get(".history-open").trigger("click");
    await flushPromises();
    expect(router.currentRoute.value.params.sessionId).toBe("journey-session");
    expect(wrapper.text()).toContain("愚者");
    expect(wrapper.text()).toContain("圣杯一");
    expect(wrapper.text()).toContain("恋人");

    await wrapper.get("a[href='/history']").trigger("click");
    await flushPromises();
    await wrapper.get("button.delete-session").trigger("click");
    expect(wrapper.get("[role='alertdialog']").text()).toContain("永久删除");
    expect(nativeApi.deleteSession).not.toHaveBeenCalled();
    await wrapper.get("button.confirm-delete").trigger("click");
    await flushPromises();
    expect(nativeApi.deleteSession).toHaveBeenCalledWith("journey-session");
    expect(wrapper.text()).toContain("还没有保存的牌阵");
    wrapper.unmount();
  });
});
