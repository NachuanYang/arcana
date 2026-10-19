import { createAuthStore } from "./auth-store";
import { bindAuthEvents, tauriAuthApi } from "./tauri-auth-api";

export const authFlow = createAuthStore(tauriAuthApi);

let initialization: Promise<void> | undefined;

export function initializeAuthFlow(): Promise<void> {
  if (!initialization) {
    initialization = bindAuthEvents(
      authFlow.acceptStatus,
      authFlow.acceptModels,
      authFlow.refreshStatus,
    ).then(() => undefined);
  }
  return initialization.then(() => authFlow.refreshStatus());
}
