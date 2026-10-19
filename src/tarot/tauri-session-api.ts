import { invoke } from "@tauri-apps/api/core";
import type { TarotSessionSnapshot, TarotSessionSummary } from "./tarot-draw-store";

export interface TarotSessionApi {
  getSession(sessionId: string): Promise<TarotSessionSnapshot | null>;
  listSessions(): Promise<TarotSessionSummary[]>;
  deleteSession(sessionId: string): Promise<boolean>;
}

export const tauriTarotSessionApi: TarotSessionApi = {
  getSession: (sessionId) => invoke<TarotSessionSnapshot | null>("get_tarot_session", { sessionId }),
  listSessions: () => invoke<TarotSessionSummary[]>("list_tarot_sessions"),
  deleteSession: (sessionId) => invoke<boolean>("delete_tarot_session", { sessionId }),
};
