import { computed, reactive } from "vue";

export interface AuthStatus {
  connected: boolean;
  planUsageAuthorized: boolean;
}

export interface AvailableModel {
  slug: string;
  displayName: string;
}

export type ProxyMode = "system" | "direct";

type AuthPreferences = { selectedModel: string; proxyMode: ProxyMode };
type PreferenceStorage = Pick<Storage, "getItem" | "setItem">;
const AUTH_PREFERENCES_KEY = "arcanum.ai-preferences.v1";

export interface AuthApi {
  getStatus(): Promise<AuthStatus>;
  signIn(): Promise<AuthStatus>;
  cancelSignIn(): Promise<boolean>;
  listModels(proxyMode: ProxyMode): Promise<AvailableModel[]>;
  testInference(model: string, proxyMode: ProxyMode): Promise<{ outputText: string; responseId: string | null }>;
  signOut(): Promise<unknown>;
  openUsage(): Promise<void>;
}

export function createAuthStore(api: AuthApi, preferenceStorage: PreferenceStorage | undefined = browserPreferenceStorage()) {
  const preferences = loadPreferences(preferenceStorage);
  const state = reactive({
    status: { connected: false, planUsageAuthorized: false } as AuthStatus,
    models: [] as AvailableModel[],
    selectedModel: preferences.selectedModel,
    proxyMode: preferences.proxyMode,
    busy: false,
    error: "",
    message: "",
    cancelRequested: false,
    activeView: "reading" as "reading" | "history" | "settings",
    lastProbe: "",
  });
  const canInfer = computed(
    () => state.status.connected && state.status.planUsageAuthorized && !state.busy && state.selectedModel.length > 0,
  );

  function acceptStatus(status: AuthStatus) {
    state.status = status;
    if (!status.connected || !status.planUsageAuthorized) {
      state.models = [];
    }
  }

  function setProxyMode(mode: ProxyMode) {
    state.proxyMode = mode;
    persistPreferences();
  }

  function selectModel(model: string) {
    if (!state.models.some((candidate) => candidate.slug === model)) return;
    state.selectedModel = model;
    persistPreferences();
  }

  function acceptModels(models: AvailableModel[]) {
    state.models = models;
    if (!models.some((model) => model.slug === state.selectedModel)) {
      state.selectedModel = models[0]?.slug ?? "";
    }
    persistPreferences();
  }

  async function refreshStatus() {
    try {
      acceptStatus(await api.getStatus());
      state.error = "";
    } catch (error) {
      state.error = explain(error);
    }
  }

  async function signIn() {
    state.busy = true;
    state.error = "";
    state.message = "请在系统浏览器中登录并确认秘仪 · ARCANUM的ChatGPT计划用量授权。";
    try {
      acceptStatus(await api.signIn());
      state.message = state.status.planUsageAuthorized
        ? "ChatGPT已连接；正在刷新当前账号可用模型。"
        : "身份登录完成，但账号尚未授予直接计划用量权限；推理功能保持关闭。";
      if (state.status.connected && state.status.planUsageAuthorized) {
        await refreshModels();
      }
    } catch (error) {
      if (state.cancelRequested) {
        state.error = "";
        state.message = "ChatGPT授权流程已取消。";
      } else {
        state.error = explain(error);
        state.message = "";
      }
    } finally {
      state.busy = false;
      state.cancelRequested = false;
    }
  }

  async function cancelSignIn() {
    if (!state.busy || state.status.connected) return;
    state.cancelRequested = true;
    try {
      const accepted = await api.cancelSignIn();
      state.message = accepted ? "已向本机授权流程发送取消请求。" : "当前没有等待中的浏览器授权流程。";
      if (!accepted) state.cancelRequested = false;
    } catch (error) {
      state.cancelRequested = false;
      state.error = explain(error);
    }
  }

  async function refreshModels() {
    if (!state.status.connected || !state.status.planUsageAuthorized) {
      state.error = "请先完成ChatGPT计划用量授权。";
      return;
    }
    state.busy = true;
    state.error = "";
    try {
      acceptModels(await api.listModels(state.proxyMode));
      if (state.models.length === 0) {
        state.message = "账号当前没有可供秘仪使用的模型，请检查账号资格或稍后重试。";
      } else {
        state.message = `已载入${state.models.length}个账号可用模型。`;
      }
    } catch (error) {
      state.error = explain(error);
      state.message = "模型目录未更新。你可以切换代理方式后重试。";
    } finally {
      state.busy = false;
    }
  }

  async function runInference() {
    if (!canInfer.value) return;
    state.busy = true;
    state.error = "";
    state.message = "正在向OpenAI发送短验证请求。";
    try {
      const result = await api.testInference(state.selectedModel, state.proxyMode);
      state.lastProbe = result.outputText;
      state.message = result.responseId
        ? `收到完整响应（${result.responseId}）。`
        : "收到完整响应。";
    } catch (error) {
      state.error = explain(error);
      state.message = "授权验证未完成。可切换代理方式或刷新授权后重试。";
    } finally {
      state.busy = false;
    }
  }

  async function signOut() {
    state.busy = true;
    state.error = "";
    try {
      await api.signOut();
      acceptStatus({ connected: false, planUsageAuthorized: false });
      state.message = "ChatGPT已断开；本地页面状态仍保留。";
    } catch (error) {
      state.error = explain(error);
    } finally {
      state.busy = false;
    }
  }

  async function openUsage() {
    try {
      await api.openUsage();
    } catch (error) {
      state.error = explain(error);
    }
  }

  return { state, canInfer, acceptStatus, acceptModels, setProxyMode, selectModel, refreshStatus, signIn, cancelSignIn, refreshModels, runInference, signOut, openUsage };

  function persistPreferences(): void {
    if (!preferenceStorage) return;
    try {
      preferenceStorage.setItem(AUTH_PREFERENCES_KEY, JSON.stringify({
        selectedModel: state.selectedModel,
        proxyMode: state.proxyMode,
      } satisfies AuthPreferences));
    } catch {
      // Local preferences are optional; failed persistence must not block readings.
    }
  }
}

function browserPreferenceStorage(): PreferenceStorage | undefined {
  try {
    return typeof window === "undefined" ? undefined : window.localStorage;
  } catch {
    return undefined;
  }
}

function loadPreferences(storage: PreferenceStorage | undefined): AuthPreferences {
  const defaults: AuthPreferences = { selectedModel: "", proxyMode: "system" };
  if (!storage) return defaults;
  try {
    const raw = storage.getItem(AUTH_PREFERENCES_KEY);
    if (!raw) return defaults;
    const value: unknown = JSON.parse(raw);
    if (!value || typeof value !== "object") return defaults;
    const candidate = value as Partial<AuthPreferences>;
    return {
      selectedModel: typeof candidate.selectedModel === "string" && candidate.selectedModel.length <= 128
        ? candidate.selectedModel
        : "",
      proxyMode: candidate.proxyMode === "direct" ? "direct" : "system",
    };
  } catch {
    return defaults;
  }
}

function explain(error: unknown): string {
  if (typeof error === "string") return error;
  return error instanceof Error ? error.message : "请求失败，请稍后重试。";
}
