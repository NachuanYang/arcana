import { createTarotSessionStore } from "./tarot-session-store";
import { tauriTarotSessionApi } from "./tauri-session-api";

export const tarotSessionStore = createTarotSessionStore(tauriTarotSessionApi);
