export interface ViewportSnapshot {
  top: number;
  wasNearBottom: boolean;
}

const nearBottomThreshold = 120;
let pendingViewport: { sessionId: string; viewport: ViewportSnapshot } | null = null;

export function captureViewportSnapshot(): ViewportSnapshot {
  const top = window.scrollY;
  const distanceToBottom = document.documentElement.scrollHeight - (top + window.innerHeight);
  return { top, wasNearBottom: distanceToBottom <= nearBottomThreshold };
}

export function rememberAutoReadViewport(sessionId: string, viewport = captureViewportSnapshot()): void {
  pendingViewport = { sessionId, viewport: { ...viewport } };
}

export function takeAutoReadViewport(sessionId: string): ViewportSnapshot | null {
  if (pendingViewport?.sessionId !== sessionId) return null;
  const viewport = pendingViewport.viewport;
  pendingViewport = null;
  return viewport;
}

export function clearAutoReadViewport(): void {
  pendingViewport = null;
}
