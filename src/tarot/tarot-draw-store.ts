import { computed, reactive } from "vue";

export type DrawBack = { position: number };
export type DrawOffer = {
  drawId: string;
  question: string;
  backs: DrawBack[];
  backImagePath: string;
};

export type TarotCardSnapshot = {
  id: string;
  arcana: string;
  number?: number | null;
  suit?: string | null;
  rank?: string | null;
  name_zh: string;
  name_en: string;
  symbol: string;
  upright_keywords: string[];
  reversed_keywords: string[];
  imagePath: string;
};

export type DrawnCard = {
  slot: "present" | "obstacle" | "tendency";
  deckCardId: string;
  orientation: "upright" | "reversed";
  visualOrder: number;
  card: TarotCardSnapshot;
};

export type RevealSnapshot = {
  drawId: string;
  datasetVersion: string;
  question: string;
  cards: DrawnCard[];
};

export type TarotTurn = {
  id: string;
  sessionId: string;
  turnOrder: number;
  role: "user" | "assistant";
  requestId: string | null;
  contentJson: string;
  status: string;
  modelId: string | null;
  createdAt: string;
};

export type TarotSessionSnapshot = {
  id: string;
  question: string;
  spreadId: string;
  deckVersion: string;
  state: string;
  createdAt: string;
  revealedAt: string;
  updatedAt: string;
  readingStance: string | null;
  cards: DrawnCard[];
  turns: TarotTurn[];
};

export type TarotSessionSummary = {
  id: string;
  question: string;
  state: string;
  createdAt: string;
  revealedAt: string;
  cards: DrawnCard[];
  turnCount: number;
};

export interface TarotDrawApi {
  createDraw(question: string): Promise<DrawOffer>;
  revealDraw(drawId: string, selectedPositions: number[]): Promise<RevealSnapshot>;
  discardDraw(drawId: string): Promise<boolean>;
}

const MIN_QUESTION_CHARACTERS = 5;
const MAX_QUESTION_CHARACTERS = 220;
const DRAW_BACK_COUNT = 12;

export function effectiveQuestionLength(question: string): number {
  return Array.from(question.trim()).filter((character) => !/\s/u.test(character)).length;
}

export function createTarotDrawStore(api: TarotDrawApi) {
  const state = reactive({
    question: "",
    offer: null as DrawOffer | null,
    selectedPositions: [] as number[],
    revealed: null as RevealSnapshot | null,
    busy: false,
    error: "",
  });
  const canReveal = computed(() =>
    Boolean(state.offer)
    && state.selectedPositions.length === 3
    && !state.busy
    && state.revealed === null,
  );

  async function start(question: string): Promise<DrawOffer | null> {
    if (state.busy) return null;
    state.question = question;
    const normalizedQuestion = question.trim();
    const length = effectiveQuestionLength(normalizedQuestion);
    if (length < MIN_QUESTION_CHARACTERS || length > MAX_QUESTION_CHARACTERS) {
      state.error = "问题需为5–220个有效字符，空格不计；输入内容已保留。";
      return null;
    }

    state.busy = true;
    state.error = "";
    try {
      if (state.offer) await api.discardDraw(state.offer.drawId);
      const offer = await api.createDraw(normalizedQuestion);
      if (offer.backs.length !== DRAW_BACK_COUNT || offer.backs.some((back, index) => back.position !== index)) {
        throw new Error("本地抽牌服务未返回完整的十二张牌背。");
      }
      if (offer.question !== normalizedQuestion || offer.backImagePath !== "/assets/tarot/back.svg") {
        throw new Error("本地抽牌服务返回的问题或牌背不匹配。");
      }
      state.question = offer.question;
      state.offer = offer;
      state.selectedPositions = [];
      state.revealed = null;
      return offer;
    } catch (error) {
      state.error = error instanceof Error ? error.message : String(error);
      return null;
    } finally {
      state.busy = false;
    }
  }

  function togglePosition(position: number): void {
    if (state.busy || state.revealed || !state.offer || !Number.isInteger(position) || position < 0 || position >= DRAW_BACK_COUNT) return;
    const existing = state.selectedPositions.indexOf(position);
    if (existing >= 0) {
      state.selectedPositions.splice(existing, 1);
    } else if (state.selectedPositions.length < 3) {
      state.selectedPositions.push(position);
    }
  }

  async function reveal(): Promise<RevealSnapshot | null> {
    if (!canReveal.value || !state.offer) return state.revealed;
    state.busy = true;
    state.error = "";
    try {
      const snapshot = await api.revealDraw(state.offer.drawId, [...state.selectedPositions]);
      if (snapshot.drawId !== state.offer.drawId || snapshot.question !== state.offer.question || snapshot.cards.length !== 3) {
        throw new Error("揭牌结果与当前固定牌阵不匹配，已保留原选择。");
      }
      if (snapshot.cards.some((card, index) => card.visualOrder !== index || card.slot !== ["present", "obstacle", "tendency"][index])) {
        throw new Error("揭牌服务返回了无效牌位，已保留原选择。");
      }
      state.revealed = snapshot;
      return snapshot;
    } catch (error) {
      state.error = error instanceof Error ? error.message : String(error);
      return null;
    } finally {
      state.busy = false;
    }
  }

  async function cancel(): Promise<void> {
    if (state.busy) return;
    state.busy = true;
    state.error = "";
    try {
      if (state.offer && !state.revealed) await api.discardDraw(state.offer.drawId);
      state.offer = null;
      state.selectedPositions = [];
      state.revealed = null;
    } catch (error) {
      state.error = error instanceof Error ? error.message : String(error);
    } finally {
      state.busy = false;
    }
  }

  return { state, canReveal, start, togglePosition, reveal, cancel };
}
