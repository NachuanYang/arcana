import { reactive } from "vue";

export type TarotCatalogCard = {
  id: string;
  arcana: "major" | "minor";
  number?: number;
  suit?: "wands" | "cups" | "swords" | "pentacles";
  rank?: string;
  name_zh: string;
  name_en: string;
  symbol: string;
  upright_keywords: string[];
  reversed_keywords: string[];
  imagePath: string;
};

export interface TarotCatalogApi {
  listCards(): Promise<TarotCatalogCard[]>;
}

export function createTarotCatalogStore(api: TarotCatalogApi) {
  const state = reactive({
    cards: [] as TarotCatalogCard[],
    loading: false,
    loaded: false,
    error: "",
  });
  let pending: Promise<void> | null = null;

  function load(): Promise<void> {
    if (state.loaded) return Promise.resolve();
    if (pending) return pending;
    state.loading = true;
    state.error = "";
    pending = Promise.resolve()
      .then(() => api.listCards())
      .then((cards) => {
        if (cards.length !== 78) throw new Error(`本地牌库数量异常：${cards.length}/78`);
        state.cards = cards;
        state.loaded = true;
      })
      .catch((error: unknown) => {
        state.error = error instanceof Error ? error.message : String(error);
      })
      .finally(() => {
        state.loading = false;
        pending = null;
      });
    return pending;
  }

  return { state, load };
}
