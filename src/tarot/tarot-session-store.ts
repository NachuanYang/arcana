import { reactive } from "vue";
import type { TarotSessionApi } from "./tauri-session-api";
import type { TarotSessionSnapshot, TarotSessionSummary } from "./tarot-draw-store";

export function createTarotSessionStore(api: TarotSessionApi) {
  const state = reactive({
    sessions: [] as TarotSessionSummary[],
    current: null as TarotSessionSnapshot | null,
    busy: false,
    error: "",
  });

  async function refresh(): Promise<void> {
    if (state.busy) return;
    state.busy = true;
    state.error = "";
    try {
      state.sessions = await api.listSessions();
    } catch (error) {
      state.error = error instanceof Error ? error.message : String(error);
    } finally {
      state.busy = false;
    }
  }

  async function open(sessionId: string): Promise<TarotSessionSnapshot | null> {
    state.current = null;
    state.busy = true;
    state.error = "";
    try {
      const session = await api.getSession(sessionId);
      if (!session) {
        state.error = "找不到这条本地牌阵记录，它可能已被删除。";
        return null;
      }
      if (session.id !== sessionId || session.cards.length !== 3) {
        state.error = "本地牌阵快照无效，已停止显示。";
        return null;
      }
      const slots = ["present", "obstacle", "tendency"];
      const validStates = new Set([
        "REVEALED", "READING_PENDING", "READING_STREAMING", "READING_READY",
        "READING_FAILED_RETRYABLE", "READING_CANCELLED", "FOLLOWUP_PENDING",
        "FOLLOWUP_STREAMING", "ARCHIVED",
      ]);
      const cardIds = session.cards.map((card) => card.deckCardId);
      if (
        session.deckVersion !== "rws_pam_a_taionwc_v1"
        || session.spreadId !== "three_present_obstacle_tendency"
        || !validStates.has(session.state)
        || session.cards.some((card, index) =>
          card.slot !== slots[index]
          || card.visualOrder !== index
          || card.deckCardId !== card.card.id
          || !["upright", "reversed"].includes(card.orientation))
        || new Set(cardIds).size !== 3
      ) {
        state.error = "本地牌阵快照无效，已停止显示。";
        return null;
      }
      state.current = session;
      return session;
    } catch (error) {
      state.error = error instanceof Error ? error.message : String(error);
      return null;
    } finally {
      state.busy = false;
    }
  }

  async function deleteSession(sessionId: string): Promise<boolean> {
    state.busy = true;
    state.error = "";
    try {
      const deleted = await api.deleteSession(sessionId);
      if (!deleted) {
        state.error = "这条牌阵记录已不存在。";
        return false;
      }
      state.sessions = state.sessions.filter((session) => session.id !== sessionId);
      if (state.current?.id === sessionId) state.current = null;
      return true;
    } catch (error) {
      state.error = error instanceof Error ? error.message : String(error);
      return false;
    } finally {
      state.busy = false;
    }
  }

  return { state, refresh, open, delete: deleteSession };
}
