import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { readingApi } from "./reading-api";

describe("readingApi configured account authorization", () => {
  beforeEach(() => invoke.mockReset());

  it("uses the configured account authorization for an initial reading", async () => {
    await readingApi.start("session", "request", "model", "system");

    expect(invoke).toHaveBeenCalledWith("start_tarot_reading", {
      request: {
        sessionId: "session",
        requestId: "request",
        model: "model",
        proxyMode: "system",
        consentToSend: true,
      },
    });
  });

  it("uses the configured account authorization for a follow-up", async () => {
    await readingApi.startFollowup("session", "request", "follow-up", "model", "direct");

    expect(invoke).toHaveBeenCalledWith("start_tarot_followup", {
      request: {
        sessionId: "session",
        requestId: "request",
        question: "follow-up",
        model: "model",
        proxyMode: "direct",
        consentToSend: true,
      },
    });
  });
});
