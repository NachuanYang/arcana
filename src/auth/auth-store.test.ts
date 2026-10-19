import { describe, expect, it, vi } from "vitest";
import { createAuthStore, type AuthApi } from "./auth-store";

const disconnected = { connected: false, planUsageAuthorized: false };
const connected = { connected: true, planUsageAuthorized: true };

function makeApi(overrides: Partial<AuthApi> = {}): AuthApi {
  return {
    getStatus: vi.fn().mockResolvedValue(disconnected),
    signIn: vi.fn().mockResolvedValue(connected),
    cancelSignIn: vi.fn().mockResolvedValue(true),
    listModels: vi.fn().mockResolvedValue([{ slug: "gpt-fast", displayName: "GPT Fast" }]),
    testInference: vi.fn().mockResolvedValue({ outputText: "ARCANUM_AUTH_OK", responseId: "resp-test" }),
    signOut: vi.fn().mockResolvedValue(undefined),
    openUsage: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

describe("ChatGPT authorization state", () => {
  it("restores the selected model and network route from local app preferences", async () => {
    localStorage.clear();
    const store = createAuthStore(makeApi());
    store.acceptStatus(connected);
    await store.refreshModels();
    store.setProxyMode("direct");

    const restored = createAuthStore(makeApi());
    expect(restored.state.selectedModel).toBe("gpt-fast");
    expect(restored.state.proxyMode).toBe("direct");
    expect(localStorage.getItem("arcanum.ai-preferences.v1")).not.toContain("access_token");
    localStorage.clear();
  });

  it("starts disconnected and cannot infer without direct plan usage permission", () => {
    const store = createAuthStore(makeApi());

    expect(store.state.status).toEqual(disconnected);
    expect(store.canInfer.value).toBe(false);
  });

  it("refreshes the signed-in state in the current window as soon as sign-in resolves", async () => {
    const api = makeApi();
    const store = createAuthStore(api);

    await store.signIn();
    expect(store.state.status).toEqual(connected);

    expect(store.state.status).toEqual(connected);
    expect(api.listModels).toHaveBeenCalledWith("system");
    expect(store.state.models).toHaveLength(1);
    expect(store.canInfer.value).toBe(true);
  });

  it("can cancel the pending native browser authorization flow", async () => {
    const store = createAuthStore(makeApi());
    store.state.busy = true;

    await store.cancelSignIn();

    expect(store.state.message).toContain("取消");
  });

  it("lets a timed-out model catalog request recover after switching to direct networking", async () => {
    const api = makeApi({
      listModels: vi.fn()
        .mockRejectedValueOnce(new Error("timeout"))
        .mockResolvedValueOnce([{ slug: "gpt-fast", displayName: "GPT Fast" }]),
    });
    const store = createAuthStore(api);
    store.acceptStatus(connected);

    await store.refreshModels();
    expect(store.state.models).toEqual([]);
    expect(store.state.error).toBeTruthy();
    store.setProxyMode("direct");
    await store.refreshModels();

    expect(api.listModels).toHaveBeenLastCalledWith("direct");
    expect(store.state.models).toHaveLength(1);
    expect(store.state.error).toBe("");
  });

  it("does not call inference while disconnected", async () => {
    const api = makeApi();
    const store = createAuthStore(api);

    await store.runInference();

    expect(api.testInference).not.toHaveBeenCalled();
  });

  it("keeps the native inference failure reason visible instead of replacing it with a generic message", async () => {
    const api = makeApi({
      testInference: vi.fn().mockRejectedValue("ChatGPT请求超时；可重试，或检查网络与系统代理。"),
    });
    const store = createAuthStore(api);
    store.acceptStatus(connected);
    await store.refreshModels();

    await store.runInference();

    expect(store.state.error).toBe("ChatGPT请求超时；可重试，或检查网络与系统代理。");
    expect(store.state.busy).toBe(false);
    expect(store.canInfer.value).toBe(true);
  });

  it("keeps the verification progress message concise", async () => {
    let resolveProbe!: (value: { outputText: string; responseId: string | null }) => void;
    const api = makeApi({
      testInference: vi.fn(() => new Promise<{ outputText: string; responseId: string | null }>((resolve) => { resolveProbe = resolve; })),
    });
    const store = createAuthStore(api);
    store.acceptStatus(connected);
    await store.refreshModels();

    const pending = store.runInference();
    expect(store.state.message).toBe("正在向OpenAI发送短验证请求。");
    resolveProbe({ outputText: "ARCANUM_AUTH_OK", responseId: "resp-test" });
    await pending;
  });

  it("sign-out leaves the current local view untouched", async () => {
    const store = createAuthStore(makeApi());
    store.acceptStatus(connected);
    store.state.activeView = "history";

    await store.signOut();

    expect(store.state.status).toEqual(disconnected);
    expect(store.state.activeView).toBe("history");
  });
});
