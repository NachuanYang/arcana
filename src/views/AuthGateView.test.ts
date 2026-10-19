import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createMemoryHistory } from "vue-router";
import { createAppRouter } from "../router";
import AuthGateView from "./AuthGateView.vue";

const { authFlow, initializeAuthFlow } = vi.hoisted(() => ({
  authFlow: {
    state: {
      status: { connected: false, planUsageAuthorized: false },
      busy: false,
      error: "",
      message: "",
      models: [] as { slug: string; displayName: string }[],
      selectedModel: "",
      proxyMode: "system",
      lastProbe: null,
    },
    signIn: vi.fn(),
    signOut: vi.fn(),
    cancelSignIn: vi.fn(),
    setProxyMode: vi.fn(),
    selectModel: vi.fn(),
    refreshModels: vi.fn(),
    runInference: vi.fn(),
    openUsage: vi.fn(),
    canInfer: { value: false },
  },
  initializeAuthFlow: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../auth/auth-flow", () => ({ authFlow, initializeAuthFlow }));

describe("AuthGateView", () => {
  beforeEach(() => {
    initializeAuthFlow.mockClear();
    authFlow.signIn.mockClear();
    authFlow.runInference.mockClear();
    Object.assign(authFlow.state, {
      status: { connected: false, planUsageAuthorized: false },
      busy: false,
      models: [],
      selectedModel: "",
      proxyMode: "system",
      error: "",
      message: "",
      lastProbe: null,
    });
    authFlow.canInfer.value = false;
  });

  it("omits the screenshot-marked hero and connection explanations while retaining authorization controls", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/settings/connection");
    const wrapper = mount(AuthGateView, { global: { plugins: [router] } });
    await flushPromises();

    expect(wrapper.find(".intro-copy").exists()).toBe(false);
    expect(wrapper.find(".edition-note").exists()).toBe(false);
    expect(wrapper.find(".card-copy").exists()).toBe(false);
    expect(wrapper.find(".permission-note").exists()).toBe(false);
    expect(wrapper.find(".catalog-entry").exists()).toBe(true);
    expect(wrapper.text()).toContain("连接ChatGPT");
    expect(wrapper.text()).toContain("使用ChatGPT账号连接");
    wrapper.unmount();
  });

  it("starts the settings verification directly without a separate consent checkbox", async () => {
    authFlow.state.status = { connected: true, planUsageAuthorized: true };
    authFlow.state.models = [{ slug: "gpt-6-luna", displayName: "GPT-6-Luna" }];
    authFlow.state.selectedModel = "gpt-6-luna";
    authFlow.canInfer.value = true;

    const router = createAppRouter(createMemoryHistory());
    await router.push("/settings/connection");
    const wrapper = mount(AuthGateView, { global: { plugins: [router] } });
    await flushPromises();

    expect(wrapper.find(".consent-check").exists()).toBe(false);
    expect(wrapper.text()).not.toContain("我同意发送一条短验证文字");
    const verifyButton = wrapper.get(".probe-button");
    expect((verifyButton.element as HTMLButtonElement).disabled).toBe(false);
    await verifyButton.trigger("click");
    expect(authFlow.runInference).toHaveBeenCalledOnce();
    wrapper.unmount();
  });

  it("keeps the waiting authorization state free of a cancel action", async () => {
    authFlow.state.busy = true;
    const router = createAppRouter(createMemoryHistory());
    await router.push("/settings/connection");
    const wrapper = mount(AuthGateView, { global: { plugins: [router] } });
    await flushPromises();

    expect(wrapper.text()).toContain("等待系统浏览器授权");
    expect(wrapper.text()).not.toContain("取消授权流程");
    expect(wrapper.findAll(".button-row button")).toHaveLength(1);
    wrapper.unmount();
  });
});
