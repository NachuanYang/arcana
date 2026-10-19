import { invoke } from "@tauri-apps/api/core";
import type { TarotCatalogApi, TarotCatalogCard } from "./tarot-catalog";

export const tauriTarotCatalogApi: TarotCatalogApi = {
  listCards: () => invoke<TarotCatalogCard[]>("list_tarot_cards"),
};
