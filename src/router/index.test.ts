import { createMemoryHistory } from "vue-router";
import { describe, expect, it } from "vitest";
import { createAppRouter } from "./index";

describe("application routing", () => {
  it("routes the desktop start to question entry and settings to official auth", async () => {
    const router = createAppRouter(createMemoryHistory());

    await router.push("/");
    expect(router.currentRoute.value.name).toBe("reading-room");
    await router.push("/settings/connection");
    expect(router.currentRoute.value.name).toBe("connection-settings");
  });

  it("uses a recoverable not-found route for unknown locations", async () => {
    const router = createAppRouter(createMemoryHistory());

    await router.push("/unexpected/path");

    expect(router.currentRoute.value.name).toBe("not-found");
  });

  it("opens the local 78-card catalog as a secondary page", async () => {
    const router = createAppRouter(createMemoryHistory());

    await router.push("/catalog");

    expect(router.currentRoute.value.name).toBe("card-catalog");
    expect(router.currentRoute.value.meta.navSection).toBe("settings");
    expect(router.currentRoute.value.meta.parentNavigation).toEqual({ to: "/settings/connection", label: "返回设置" });
  });

  it("routes history lists and saved read-only sessions", async () => {
    const router = createAppRouter(createMemoryHistory());

    await router.push("/history");
    expect(router.currentRoute.value.name).toBe("history");
    await router.push("/reading/saved-session");
    expect(router.currentRoute.value.name).toBe("session-detail");
    expect(router.currentRoute.value.meta.navSection).toBe("history");
    expect(router.currentRoute.value.meta.parentNavigation).toEqual({ to: "/history", label: "返回历史牌阵" });
    await router.push("/history/saved-session");
    expect(router.currentRoute.value.name).toBe("history-session");
    expect(router.currentRoute.value.meta.navSection).toBe("history");
    expect(router.currentRoute.value.meta.parentNavigation).toEqual({ to: "/history", label: "返回历史牌阵" });
  });

  it("routes a pending opaque draw to the selection screen", async () => {
    const router = createAppRouter(createMemoryHistory());

    await router.push("/reading/draw/opaque-draw");

    expect(router.currentRoute.value.name).toBe("draw-view");
    expect(router.currentRoute.value.params.drawId).toBe("opaque-draw");
    expect(router.currentRoute.value.meta.parentNavigation).toEqual({ to: "/", label: "返回新占卜" });
  });
});
