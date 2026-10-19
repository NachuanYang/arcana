import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ProxyMode } from "../auth/auth-store";
import type { ReadingResult } from "./reading-view-model";
import type { FollowupResult } from "./followup-view-model";

export type ReadingProgress = {
  sessionId: string;
  requestId: string;
  state: string;
};

export const readingApi = {
  start(sessionId: string, requestId: string, model: string, proxyMode: ProxyMode): Promise<ReadingResult> {
    return invoke<ReadingResult>("start_tarot_reading", {
      request: { sessionId, requestId, model, proxyMode, consentToSend: true },
    });
  },
  cancel(sessionId: string, requestId: string): Promise<boolean> {
    return invoke<boolean>("cancel_tarot_reading", { sessionId, requestId });
  },
  startFollowup(
    sessionId: string,
    requestId: string,
    question: string,
    model: string,
    proxyMode: ProxyMode,
  ): Promise<FollowupResult> {
    return invoke<FollowupResult>("start_tarot_followup", {
      request: { sessionId, requestId, question, model, proxyMode, consentToSend: true },
    });
  },
  cancelFollowup(sessionId: string, requestId: string): Promise<boolean> {
    return invoke<boolean>("cancel_tarot_followup", { sessionId, requestId });
  },
  listenProgress(handler: (progress: ReadingProgress) => void): Promise<UnlistenFn> {
    return listen<ReadingProgress>("reading://progress", (event) => handler(event.payload));
  },
};
