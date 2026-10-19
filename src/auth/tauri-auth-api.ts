import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AuthApi, AuthStatus, AvailableModel } from "./auth-store";

export const tauriAuthApi: AuthApi = {
  getStatus: () => invoke<AuthStatus>("get_chatgpt_connection_status"),
  signIn: () => invoke<AuthStatus>("connect_chatgpt"),
  cancelSignIn: () => invoke<boolean>("cancel_chatgpt_sign_in"),
  listModels: (proxyMode) => invoke<AvailableModel[]>("list_chatgpt_models", { proxyMode }),
  testInference: (model, proxyMode) =>
    invoke<{ outputText: string; responseId: string | null }>("test_chatgpt_inference", {
      model,
      proxyMode,
    }),
  signOut: () => invoke("disconnect_chatgpt"),
  openUsage: () => invoke("open_chatgpt_usage"),
};

export async function bindAuthEvents(
  acceptStatus: (status: AuthStatus) => void,
  acceptModels: (models: AvailableModel[]) => void,
  onFocus: () => void,
): Promise<UnlistenFn> {
  const unlistenStatus = await listen<AuthStatus>("auth://status-changed", (event) => {
    acceptStatus(event.payload);
  });
  const unlistenModels = await listen<AvailableModel[]>("auth://model-catalog-updated", (event) => {
    acceptModels(event.payload);
  });
  window.addEventListener("focus", onFocus);
  return () => {
    unlistenStatus();
    unlistenModels();
    window.removeEventListener("focus", onFocus);
  };
}
