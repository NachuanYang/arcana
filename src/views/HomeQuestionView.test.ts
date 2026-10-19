import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createMemoryHistory } from "vue-router";
import { createAppRouter } from "../router";
import { tarotDrawStore } from "../tarot/tarot-draw-flow";
import HomeQuestionView from "./HomeQuestionView.vue";

const { nativeApi } = vi.hoisted(() => ({
  nativeApi: {
    createDraw: vi.fn(),
    revealDraw: vi.fn(),
    discardDraw: vi.fn(),
  },
}));

vi.mock("../tarot/tauri-tarot-draw-api", () => ({ tauriTarotDrawApi: nativeApi }));

describe("HomeQuestionView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
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
    nativeApi.revealDraw.mockReset();
    nativeApi.discardDraw.mockResolvedValue(true);
  });

  it("omits the screenshot-marked introductory, field-helper, and disclaimer copy", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/");
    const wrapper = mount(HomeQuestionView, { global: { plugins: [router] } });

    expect(wrapper.find(".question-intro > p").exists()).toBe(false);
    expect(wrapper.find(".spread-preview").exists()).toBe(false);
    expect(wrapper.find(".question-prompt").exists()).toBe(false);
    expect(wrapper.find("label[for='question-input']").exists()).toBe(false);
    expect(wrapper.find(".question-disclaimer").exists()).toBe(false);
    expect(wrapper.text()).not.toContain("尽量聚焦一个具体问题");
    expect(wrapper.find("textarea[aria-label='占卜问题']").exists()).toBe(true);
    expect(wrapper.find("textarea[placeholder]").exists()).toBe(false);
    expect(wrapper.find(".question-count").exists()).toBe(false);
    expect(wrapper.find(".question-helper").exists()).toBe(false);
    wrapper.unmount();
  });

  it("shows concise validation only after an invalid question is submitted", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/");
    const wrapper = mount(HomeQuestionView, { global: { plugins: [router] } });
    const input = wrapper.get("textarea[aria-label='占卜问题']");

    await input.setValue("项目如何");
    expect(wrapper.find(".question-validation").exists()).toBe(false);
    expect((wrapper.get("button[type='submit']").element as HTMLButtonElement).disabled).toBe(false);
    await wrapper.get("form").trigger("submit");
    expect(wrapper.get(".question-validation").text()).toContain("5至220");
    expect((input.element as HTMLTextAreaElement).value).toBe("项目如何");

    await input.setValue("项目如何走");
    expect((wrapper.get("button[type='submit']").element as HTMLButtonElement).disabled).toBe(false);
    expect(wrapper.find(".question-validation").exists()).toBe(false);
    wrapper.unmount();
  });

  it("starts each visit with an empty question instead of reusing a prior session", async () => {
    tarotDrawStore.state.question = "历史牌阵里的旧问题";
    const router = createAppRouter(createMemoryHistory());
    await router.push("/");
    const wrapper = mount(HomeQuestionView, { global: { plugins: [router] } });

    expect((wrapper.get("textarea[aria-label='占卜问题']").element as HTMLTextAreaElement).value).toBe("");
    wrapper.unmount();
  });

  it("starts one question-bound draw without exposing a conclusion", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/");
    const wrapper = mount(HomeQuestionView, { global: { plugins: [router] } });

    await wrapper.get("textarea[aria-label='占卜问题']").setValue("  项目如何走  ");
    await wrapper.get("form").trigger("submit");
    await flushPromises();

    expect(nativeApi.createDraw).toHaveBeenCalledWith("项目如何走");
    expect(router.currentRoute.value.name).toBe("draw-view");
    expect(router.currentRoute.value.params.drawId).toBe("opaque-draw-1");
    expect(wrapper.text()).not.toContain("结论");
    wrapper.unmount();
  });

  it("does not truncate overlong input and reports the limit on submit", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/");
    const wrapper = mount(HomeQuestionView, { global: { plugins: [router] } });
    const tooLong = "问".repeat(221);

    await wrapper.get("textarea[aria-label='占卜问题']").setValue(tooLong);

    expect((wrapper.get("textarea").element as HTMLTextAreaElement).value).toBe(tooLong);
    expect(wrapper.find(".question-count").exists()).toBe(false);
    expect((wrapper.get("button[type='submit']").element as HTMLButtonElement).disabled).toBe(false);
    await wrapper.get("form").trigger("submit");
    expect(wrapper.get(".question-validation").text()).toContain("5至220");
    expect(nativeApi.createDraw).not.toHaveBeenCalled();
    wrapper.unmount();
  });
});
