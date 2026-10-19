import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createMemoryHistory } from "vue-router";
import { createAppRouter } from "../router";
import { rememberAutoReadViewport } from "../router/auto-read-viewport";
import { tarotSessionStore } from "../tarot/tarot-session-flow";
import type { TarotTurn } from "../tarot/tarot-draw-store";
import SessionDetailView from "./SessionDetailView.vue";

const { nativeApi } = vi.hoisted(() => ({
  nativeApi: { getSession: vi.fn(), listSessions: vi.fn(), deleteSession: vi.fn() },
}));
const { authFlow, initializeAuthFlow } = vi.hoisted(() => ({
  authFlow: {
    state: {
      status: { connected: true, planUsageAuthorized: true },
      models: [{ slug: "gpt-test", displayName: "Test model" }],
      selectedModel: "gpt-test", proxyMode: "system", busy: false, error: "", message: "",
    },
    acceptStatus: vi.fn(), setProxyMode: vi.fn(), refreshStatus: vi.fn().mockResolvedValue(undefined),
    refreshModels: vi.fn().mockResolvedValue(undefined),
  },
  initializeAuthFlow: vi.fn().mockResolvedValue(undefined),
}));
const { startReading, cancelReading, startFollowup, cancelFollowup, listenProgress } = vi.hoisted(() => ({
  startReading: vi.fn(), cancelReading: vi.fn(), startFollowup: vi.fn(), cancelFollowup: vi.fn(), listenProgress: vi.fn().mockResolvedValue(() => undefined),
}));
let restoreViewport: (() => void) | undefined;

vi.mock("../tarot/tauri-session-api", () => ({ tauriTarotSessionApi: nativeApi }));
vi.mock("../auth/auth-flow", () => ({ authFlow, initializeAuthFlow }));
vi.mock("../reading/reading-api", () => ({ readingApi: { start: startReading, cancel: cancelReading, startFollowup, cancelFollowup, listenProgress } }));

function setViewport(scrollY: number, documentHeight: number, viewportHeight: number) {
  const originalScrollY = Object.getOwnPropertyDescriptor(window, "scrollY");
  const originalScrollHeight = Object.getOwnPropertyDescriptor(document.documentElement, "scrollHeight");
  const originalInnerHeight = Object.getOwnPropertyDescriptor(window, "innerHeight");
  const scrollTo = vi.spyOn(window, "scrollTo").mockImplementation(() => undefined);
  Object.defineProperty(window, "scrollY", { configurable: true, value: scrollY });
  Object.defineProperty(document.documentElement, "scrollHeight", { configurable: true, value: documentHeight });
  Object.defineProperty(window, "innerHeight", { configurable: true, value: viewportHeight });
  restoreViewport = () => {
    scrollTo.mockRestore();
    if (originalScrollY) Object.defineProperty(window, "scrollY", originalScrollY);
    if (originalScrollHeight) Object.defineProperty(document.documentElement, "scrollHeight", originalScrollHeight);
    if (originalInnerHeight) Object.defineProperty(window, "innerHeight", originalInnerHeight);
  };
  return scrollTo;
}

describe("SessionDetailView", () => {
  afterEach(() => {
    restoreViewport?.();
    restoreViewport = undefined;
    vi.restoreAllMocks();
  });

  beforeEach(() => {
    vi.spyOn(window, "scrollTo").mockImplementation(() => undefined);
    Object.assign(tarotSessionStore.state, { sessions: [], current: null, busy: false, error: "" });
    Object.assign(authFlow.state, {
      status: { connected: true, planUsageAuthorized: true },
      selectedModel: "gpt-test",
      proxyMode: "system",
    });
    nativeApi.getSession.mockResolvedValue({
      id: "saved-session",
      question: "项目如何走",
      spreadId: "three_present_obstacle_tendency",
      deckVersion: "rws_pam_a_taionwc_v1",
      state: "REVEALED",
      createdAt: "2026-10-12T10:00:00.000000000Z",
      revealedAt: "2026-10-12T10:00:00.000000000Z",
      updatedAt: "2026-10-12T10:00:00.000000000Z",
      readingStance: null,
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
      turns: [],
    });
    startReading.mockReset();
    startFollowup.mockReset();
    cancelFollowup.mockReset();
    startReading.mockResolvedValue({
      schemaVersion: 1,
      sessionId: "saved-session",
      stance: "CAUTIOUS",
      headline: "方向可行，但需要先处理现实阻力",
      synthesis: "现状显示你已经具备启动条件，但阻碍位提示资源和沟通仍有缺口。趋向并非自动成功；如果先补齐关键条件，推进空间会更清楚。".repeat(3),
      cards: ["present", "obstacle", "tendency"].map((slot, index) => ({
        slot,
        cardId: `major_0${index}`,
        orientation: index === 1 ? "reversed" : "upright",
        explanation: "这张牌提醒你先观察已经具备的条件，并把象征提示和能够核对的实际信息分开思考，再据此决定下一步。".repeat(2),
      })),
      tension: "推进动力与资源准备之间仍有落差。",
      actionableReflection: "先确认最关键的一项资源是否到位，再决定下一步节奏。",
      limitsNote: "这是一种象征性反思，不是对现实结果的证明。",
    });
  });

  it("restores the frozen three-card spread directly from SQLite after restart", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/reading/saved-session");
    const wrapper = mount(SessionDetailView, { attachTo: document.body, global: { plugins: [router] } });
    await flushPromises();

    expect(nativeApi.getSession).toHaveBeenCalledWith("saved-session");
    expect(wrapper.text()).toContain("项目如何走");
    expect(wrapper.findAll("article.revealed-card")).toHaveLength(3);
    const summary = wrapper.get(".session-detail-content .question-summary");
    expect(summary.text()).toBe("项目如何走");
    expect(summary.attributes("aria-label")).toBe("项目如何走");
    expect(wrapper.get(".session-detail-content").findAll(":scope > h1")).toHaveLength(0);
    expect(wrapper.find(".fixed-note").exists()).toBe(false);
    expect(wrapper.get("article.revealed-card:nth-child(2) img").classes()).toContain("reversed-art");
    expect(wrapper.text()).not.toContain("结论");
    expect(wrapper.text()).not.toContain("只读历史快照");
    expect(wrapper.find("select").exists()).toBe(false);
    expect(wrapper.find(".reading-result-headline").exists()).toBe(false);
    expect(wrapper.find(".session-actions").exists()).toBe(false);
    wrapper.unmount();
  });

  it("opens a history session as an unlimited chronological conversation with its composer ready", async () => {
    const saved = await nativeApi.getSession();
    const initialReading = await startReading("saved-session", "initial", "gpt-test", "system", true);
    const initialTurn = {
      id: "initial-answer", sessionId: saved.id, turnOrder: 1, role: "assistant" as const, requestId: "initial-request",
      contentJson: JSON.stringify(initialReading), status: "COMPLETE" as const, modelId: "gpt-test", createdAt: "",
    };
    const turns: TarotTurn[] = [initialTurn];
    for (let index = 1; index <= 7; index += 1) {
      const requestId = `request-${index}`;
      const userTurn = {
        id: `user-${index}`, sessionId: saved.id, turnOrder: index * 2, role: "user" as const, requestId,
        contentJson: JSON.stringify({ question: `历史追问${index}` }), status: "COMPLETE" as const, modelId: null, createdAt: "",
      };
      const assistantTurn = {
        id: `assistant-${index}`, sessionId: saved.id, turnOrder: index * 2 + 1, role: "assistant" as const, requestId,
        contentJson: JSON.stringify({
          schemaVersion: 1,
          sessionId: saved.id,
          replyToTurnId: userTurn.id,
          answer: `这是第${index}条追问的完整回答，依据固定牌阵说明下一步核实重点，并提醒把象征倾向与现实证据分开。`,
          citedCards: [{ slot: "obstacle", cardId: "major_01", orientation: "reversed" }],
          stanceUnchanged: true,
          contextSummary: "用户正在谨慎核实项目风险，保留审慎判断并逐步收集信息，同时确认资源、沟通进度与截止时间。",
        }), status: "COMPLETE" as const, modelId: "gpt-test", createdAt: "",
      };
      turns.push(userTurn, assistantTurn);
    }
    nativeApi.getSession.mockResolvedValue({
      ...saved,
      state: "READING_READY",
      readingStance: "CAUTIOUS",
      turns,
    });

    const router = createAppRouter(createMemoryHistory());
    await router.push("/history/saved-session");
    const wrapper = mount(SessionDetailView, { global: { plugins: [router] } });
    await flushPromises();

    const transcript = wrapper.get("[data-testid='conversation-transcript']");
    expect(transcript.text()).toContain("项目如何走");
    expect(transcript.text()).toContain("这是第7条追问的完整回答");
    expect(transcript.findAll("[data-role='user']")).toHaveLength(8);
    expect(transcript.findAll("[data-role='assistant']")).toHaveLength(8);
    expect(wrapper.text()).not.toContain("只读历史快照");
    expect(wrapper.find("select").exists()).toBe(false);
    expect((wrapper.get("#followup-question").element as HTMLTextAreaElement).disabled).toBe(false);
    expect(wrapper.find(".followup-limit-message").exists()).toBe(false);
    wrapper.unmount();
  });

  it("opens a full-card viewer that navigates only the saved session spread", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/reading/saved-session");
    const wrapper = mount(SessionDetailView, { attachTo: document.body, global: { plugins: [router] } });
    await flushPromises();
    const trigger = wrapper.findAll("button.revealed-art-frame")[0];
    (trigger.element as HTMLButtonElement).focus();
    await trigger.trigger("click");

    expect(wrapper.get('[role="dialog"] h2').text()).toContain("愚者");
    expect(wrapper.get('[role="dialog"]').text()).toContain("1 / 3");
    await wrapper.get('[role="dialog"]').trigger("keydown", { key: "ArrowRight" });
    expect(wrapper.get('[role="dialog"] h2').text()).toContain("魔术师");
    expect(wrapper.get('[role="dialog"] img').classes()).toContain("reversed-art");
    await wrapper.get('[role="dialog"]').trigger("keydown", { key: "Escape" });
    expect(wrapper.find('[role="dialog"]').exists()).toBe(false);
    expect(document.activeElement).toBe(trigger.element);
    wrapper.unmount();
  });

  it("starts an incomplete history reading without a second consent prompt and validates the response", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/reading/saved-session");
    const wrapper = mount(SessionDetailView, { global: { plugins: [router] } });
    await flushPromises();

    const runButton = wrapper.get(".reading-actions .button-primary");
    expect(wrapper.find(".reading-consent").exists()).toBe(false);
    expect((runButton.element as HTMLButtonElement).disabled).toBe(false);
    expect(startReading).not.toHaveBeenCalled();

    await runButton.trigger("click");
    await flushPromises();

    expect(startReading).toHaveBeenCalledWith("saved-session", expect.any(String), "gpt-test", "system");
    expect(wrapper.text()).toContain("方向可行，但需要先处理现实阻力");
    expect(wrapper.text()).toContain("牌间张力");
    expect(wrapper.text()).toContain("这是一种象征性反思");
    wrapper.unmount();
  });

  it("sends a same-spread follow-up without a second consent prompt and restores its cited answer", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/reading/saved-session");
    const wrapper = mount(SessionDetailView, { global: { plugins: [router] } });
    await flushPromises();

    await wrapper.get(".reading-actions .button-primary").trigger("click");
    await flushPromises();
    const firstReading = await startReading.mock.results[0].value;
    const saved = tarotSessionStore.state.current!;
    saved.state = "READING_READY";
    saved.readingStance = "CAUTIOUS";
    saved.turns = [{
      id: "initial-turn", sessionId: saved.id, turnOrder: 1, role: "assistant", requestId: "initial-request",
      contentJson: JSON.stringify(firstReading), status: "COMPLETE", modelId: "gpt-test", createdAt: "",
    }];
    await flushPromises();

    const question = "为什么这副牌建议我谨慎？";
    const followupResponse = {
      schemaVersion: 1, sessionId: saved.id, replyToTurnId: "followup-user-turn",
      answer: "这份谨慎来自阻碍位的逆位提示和趋向位仍需观察的信号，并不否定你的推进意愿。先确认资源条件，再用小步骤收集现实反馈。",
      citedCards: [{ slot: "obstacle", cardId: "major_01", orientation: "reversed" }],
      stanceUnchanged: true,
    };
    const userTurn = {
      id: "followup-user-turn", sessionId: saved.id, turnOrder: 2, role: "user", requestId: "",
      contentJson: JSON.stringify({ question }), status: "COMPLETE", modelId: null, createdAt: "",
    };
    const assistantTurn = {
      id: "followup-assistant-turn", sessionId: saved.id, turnOrder: 3, role: "assistant", requestId: "",
      contentJson: JSON.stringify(followupResponse), status: "COMPLETE", modelId: "gpt-test", createdAt: "",
    };
    startFollowup.mockImplementation(async (_sessionId: string, requestId: string) => {
      nativeApi.getSession.mockResolvedValueOnce({
        ...saved,
        turns: [
          ...saved.turns,
          { ...userTurn, requestId },
          { ...assistantTurn, requestId },
        ],
      });
      return followupResponse;
    });

    const sendButton = wrapper.get(".followup-controls .button-primary");
    const questionField = wrapper.get("#followup-question");
    expect(questionField.attributes("placeholder")).toBeUndefined();
    expect(wrapper.find(".followup-controls .consent-check").exists()).toBe(false);
    expect((sendButton.element as HTMLButtonElement).disabled).toBe(true);
    expect(startFollowup).not.toHaveBeenCalled();
    await questionField.setValue(question);
    expect((sendButton.element as HTMLButtonElement).disabled).toBe(false);
    nativeApi.getSession.mockResolvedValueOnce(saved);
    startFollowup.mockRejectedValueOnce(new Error("模拟网络中断"));
    const scrollTo = setViewport(320, 1800, 800);
    await sendButton.trigger("click");
    await flushPromises();
    expect(startFollowup).toHaveBeenCalledTimes(1);
    expect(scrollTo).toHaveBeenLastCalledWith(0, 320);
    scrollTo.mockClear();
    Object.defineProperty(window, "scrollY", { configurable: true, value: 1150 });
    await wrapper.get(".followup-controls .button-primary").trigger("click");
    await flushPromises();

    expect(startFollowup).toHaveBeenCalledTimes(2);
    expect(startFollowup).toHaveBeenCalledWith(saved.id, expect.any(String), question, "gpt-test", "system");
    expect(scrollTo).toHaveBeenLastCalledWith(0, 1000);
    expect(wrapper.text()).toContain("这份谨慎来自阻碍位");
    expect(wrapper.text()).toContain("魔术师 · 阻碍 · 逆位");
    expect(wrapper.text()).not.toContain("/5");
    expect(wrapper.find(".reading-progress").exists()).toBe(false);
    expect(wrapper.text()).not.toContain("追问已保存到本机。");
    wrapper.unmount();
  });

  it("does not show a separate cancel button while a follow-up is processing", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/reading/saved-session");
    const wrapper = mount(SessionDetailView, { global: { plugins: [router] } });
    await flushPromises();

    await wrapper.get(".reading-actions .button-primary").trigger("click");
    await flushPromises();
    const reading = await startReading.mock.results[0].value;
    const saved = tarotSessionStore.state.current!;
    saved.state = "READING_READY";
    saved.readingStance = "CAUTIOUS";
    saved.turns = [{
      id: "initial-turn", sessionId: saved.id, turnOrder: 1, role: "assistant", requestId: "initial-request",
      contentJson: JSON.stringify(reading), status: "COMPLETE", modelId: "gpt-test", createdAt: "",
    }];
    startFollowup.mockImplementation(() => new Promise(() => undefined));
    cancelFollowup.mockResolvedValue(true);

    await wrapper.get("#followup-question").setValue("我应该先核实哪件事？");
    await wrapper.get(".followup-controls .button-primary").trigger("click");
    await flushPromises();

    expect(wrapper.get(".followup-controls .button-primary").text()).toContain("正在处理追问");
    expect(wrapper.find(".followup-controls .button-secondary").exists()).toBe(false);
    wrapper.unmount();
  });

  it("shows string errors returned by the Tauri follow-up command", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/reading/saved-session");
    const wrapper = mount(SessionDetailView, { global: { plugins: [router] } });
    await flushPromises();

    await wrapper.get(".reading-actions .button-primary").trigger("click");
    await flushPromises();
    const reading = await startReading.mock.results[0].value;
    const saved = tarotSessionStore.state.current!;
    saved.state = "READING_READY";
    saved.readingStance = "CAUTIOUS";
    const initialTurn = {
      id: "initial-turn", sessionId: saved.id, turnOrder: 1, role: "assistant" as const, requestId: "initial-request",
      contentJson: JSON.stringify(reading), status: "COMPLETE" as const, modelId: "gpt-test", createdAt: "",
    };
    saved.turns = [initialTurn];
    await flushPromises();

    const question = "我应该优先核实哪些信息？";
    const providerError = "OpenAI HTTP 429：账号计划用量或请求频率受限。";
    startFollowup.mockImplementationOnce(async (_sessionId: string, requestId: string) => {
      const userTurn = {
        id: "followup-user-turn", sessionId: saved.id, turnOrder: 2, role: "user" as const, requestId,
        contentJson: JSON.stringify({ question }), status: "COMPLETE" as const, modelId: null, createdAt: "",
      };
      const assistantTurn = {
        id: "followup-assistant-turn", sessionId: saved.id, turnOrder: 3, role: "assistant" as const, requestId,
        contentJson: JSON.stringify({ stage: "followup_failed" }), status: "FAILED_RETRYABLE" as const, modelId: "gpt-test", createdAt: "",
      };
      nativeApi.getSession.mockResolvedValueOnce({ ...saved, turns: [initialTurn, userTurn, assistantTurn] });
      throw providerError;
    });

    await wrapper.get("#followup-question").setValue(question);
    await wrapper.get(".followup-controls .button-primary").trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain(providerError);
    expect(wrapper.text()).not.toContain("追问未完成，可调整网络或授权后重试。");
    expect(wrapper.text()).not.toContain("/5");
    wrapper.unmount();
  });

  it("auto-starts one initial reading only for the fresh-draw route marker", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push({ name: "session-detail", params: { sessionId: "saved-session" }, query: { autoRead: "1" } });
    const scrollTo = setViewport(420, 1800, 800);
    rememberAutoReadViewport("saved-session", { top: 420, wasNearBottom: false });
    router.afterEach((to, from) => {
      if (from.query.autoRead === "1" && to.query.autoRead === undefined) {
        Object.defineProperty(window, "scrollY", { configurable: true, value: 0 });
      }
    });
    const wrapper = mount(SessionDetailView, { global: { plugins: [router] } });
    await flushPromises();

    expect(startReading).toHaveBeenCalledTimes(1);
    expect(startReading).toHaveBeenCalledWith("saved-session", expect.any(String), "gpt-test", "system");
    expect(router.currentRoute.value.query.autoRead).toBeUndefined();
    expect(wrapper.text()).toContain("方向可行，但需要先处理现实阻力");
    expect(scrollTo).toHaveBeenLastCalledWith(0, 420);
    wrapper.unmount();
  });

  it("shows an unmistakable loading state during automatic interpretation", async () => {
    let rejectReading!: (error: Error) => void;
    startReading.mockImplementationOnce(() => new Promise((_resolve, reject) => { rejectReading = reject; }));
    const router = createAppRouter(createMemoryHistory());
    await router.push({ name: "session-detail", params: { sessionId: "saved-session" }, query: { autoRead: "1" } });
    const wrapper = mount(SessionDetailView, { global: { plugins: [router] } });
    await flushPromises();

    expect(wrapper.get(".initial-reading-controls h3").text()).toBe("正在解读这组牌…");
    expect(wrapper.find(".initial-reading-controls .reading-actions .button-primary").exists()).toBe(false);
    expect(wrapper.find(".initial-reading-controls .reading-actions .button-secondary").exists()).toBe(false);
    expect(wrapper.find(".initial-reading-controls .reading-progress").exists()).toBe(false);

    rejectReading(new Error("simulated cancellation"));
    await flushPromises();
    wrapper.unmount();
  });

  it("does not auto-start when a saved session is opened from history", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/history/saved-session");
    const wrapper = mount(SessionDetailView, { global: { plugins: [router] } });
    await flushPromises();

    expect(startReading).not.toHaveBeenCalled();
    expect(startFollowup).not.toHaveBeenCalled();
    wrapper.unmount();
  });

  it("keeps a fresh-draw auto-read pending until Settings account and model are ready", async () => {
    authFlow.state.status = { connected: false, planUsageAuthorized: false };
    authFlow.state.selectedModel = "";
    const router = createAppRouter(createMemoryHistory());
    await router.push({ name: "session-detail", params: { sessionId: "saved-session" }, query: { autoRead: "1" } });
    const wrapper = mount(SessionDetailView, { global: { plugins: [router] } });
    await flushPromises();

    expect(startReading).not.toHaveBeenCalled();
    expect(router.currentRoute.value.query.autoRead).toBe("1");
    expect(wrapper.find('a[href="/settings/connection"]').exists()).toBe(true);
    wrapper.unmount();
  });
});
