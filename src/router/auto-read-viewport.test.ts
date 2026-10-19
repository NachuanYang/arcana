import { afterEach, describe, expect, it } from "vitest";
import {
  captureViewportSnapshot,
  clearAutoReadViewport,
  rememberAutoReadViewport,
  takeAutoReadViewport,
} from "./auto-read-viewport";

const originalScrollY = Object.getOwnPropertyDescriptor(window, "scrollY");
const originalScrollHeight = Object.getOwnPropertyDescriptor(document.documentElement, "scrollHeight");
const originalInnerHeight = Object.getOwnPropertyDescriptor(window, "innerHeight");

afterEach(() => {
  clearAutoReadViewport();
  if (originalScrollY) Object.defineProperty(window, "scrollY", originalScrollY);
  if (originalScrollHeight) Object.defineProperty(document.documentElement, "scrollHeight", originalScrollHeight);
  if (originalInnerHeight) Object.defineProperty(window, "innerHeight", originalInnerHeight);
});

describe("automatic reading viewport handoff", () => {
  it("captures the viewport position and whether it was near the bottom", () => {
    Object.defineProperty(window, "scrollY", { configurable: true, value: 620 });
    Object.defineProperty(document.documentElement, "scrollHeight", { configurable: true, value: 1500 });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 800 });

    expect(captureViewportSnapshot()).toEqual({ top: 620, wasNearBottom: true });
  });

  it("hands a draw viewport to its matching session once, even after the browser clamps scroll to the top", () => {
    const viewport = { top: 460, wasNearBottom: false };
    rememberAutoReadViewport("session-1", viewport);
    Object.defineProperty(window, "scrollY", { configurable: true, value: 0 });

    expect(takeAutoReadViewport("other-session")).toBeNull();
    expect(takeAutoReadViewport("session-1")).toEqual(viewport);
    expect(takeAutoReadViewport("session-1")).toBeNull();
  });
});
