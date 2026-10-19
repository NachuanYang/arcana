import { invoke } from "@tauri-apps/api/core";
import type { DrawOffer, RevealSnapshot, TarotDrawApi } from "./tarot-draw-store";

export const tauriTarotDrawApi: TarotDrawApi = {
  createDraw: (question) => invoke<DrawOffer>("create_tarot_draw", { question }),
  revealDraw: (drawId, selectedPositions) =>
    invoke<RevealSnapshot>("reveal_tarot_draw", { drawId, selectedPositions }),
  discardDraw: (drawId) => invoke<boolean>("discard_tarot_draw", { drawId }),
};
