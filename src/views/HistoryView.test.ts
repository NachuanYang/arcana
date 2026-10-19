import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createMemoryHistory } from "vue-router";
import { createAppRouter } from "../router";
import { tarotSessionStore } from "../tarot/tarot-session-flow";
import HistoryView from "./HistoryView.vue";

const { nativeApi } = vi.hoisted(() => ({
  nativeApi: {
    getSession: vi.fn(),
    listSessions: vi.fn(),
    deleteSession: vi.fn(),
  },
}));

vi.mock("../tarot/tauri-session-api", () => ({ tauriTarotSessionApi: nativeApi }));

function savedSession(id = "saved-session") {
  return {
    id,
    question: "已经保存的问题",
    spreadId: "three_present_obstacle_tendency",
    deckVersion: "rws_pam_a_taionwc_v1",
    state: "REVEALED",
    createdAt: "2026-10-12T10:00:00.000000000Z",
    revealedAt: "2026-10-12T10:00:00.000000000Z",
    updatedAt: "2026-10-12T10:00:00.000000000Z",
    readingStance: null,
    cards: [],
    turns: [],
  };
}

describe("HistoryView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Object.assign(tarotSessionStore.state, { sessions: [], current: null, busy: false, error: "" });
    nativeApi.listSessions.mockResolvedValue([{
      id: "saved-session",
      question: "已经保存的问题",
      state: "READING_READY",
      createdAt: "2026-10-12T10:00:00.000000000Z",
      revealedAt: "2026-10-12T10:00:00.000000000Z",
      cards: [],
      turnCount: 7,
    }]);
    nativeApi.getSession.mockImplementation(async (id: string) => savedSession(id));
    nativeApi.deleteSession.mockResolvedValue(true);
  });

  it("lists persistent sessions without the marked status copy and requires confirmation before deletion", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/history");
    const wrapper = mount(HistoryView, { global: { plugins: [router] } });
    await flushPromises();

    expect(wrapper.text()).toContain("已经保存的问题");
    expect(wrapper.find("a[href='/reading/saved-session']").exists()).toBe(true);
    expect(wrapper.text()).not.toContain("你的问题与固定牌面保存在本机");
    expect(wrapper.text()).not.toContain("解读已完成");
    expect(wrapper.text()).not.toContain("7条对话");
    await wrapper.get("button.delete-session").trigger("click");
    expect(wrapper.get("[role='alertdialog']").text()).toContain("永久删除");
    expect(nativeApi.deleteSession).not.toHaveBeenCalled();

    await wrapper.get("button.cancel-delete").trigger("click");
    expect(nativeApi.deleteSession).not.toHaveBeenCalled();
    await wrapper.get("button.delete-session").trigger("click");
    await wrapper.get("button.confirm-delete").trigger("click");
    await flushPromises();

    expect(nativeApi.deleteSession).toHaveBeenCalledWith("saved-session");
    expect(wrapper.text()).toContain("还没有保存的牌阵");
    wrapper.unmount();
  });

  it("keeps the empty archive state concise", async () => {
    nativeApi.listSessions.mockResolvedValueOnce([]);
    const router = createAppRouter(createMemoryHistory());
    await router.push("/history");
    const wrapper = mount(HistoryView, { global: { plugins: [router] } });
    await flushPromises();

    expect(wrapper.text()).toContain("还没有保存的牌阵");
    expect(wrapper.text()).not.toContain("揭开三张牌后，本次问题和固定牌面会保存到这里。");
    wrapper.unmount();
  });
});
