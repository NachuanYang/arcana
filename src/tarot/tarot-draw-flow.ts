import { createTarotDrawStore } from "./tarot-draw-store";
import { tauriTarotDrawApi } from "./tauri-tarot-draw-api";

export const tarotDrawStore = createTarotDrawStore(tauriTarotDrawApi);
