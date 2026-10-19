import { describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createMemoryHistory } from "vue-router";
import App from "./App.vue";
import { createAppRouter } from "./router";

describe("App shell", () => {
  it("keeps the same three navigation links and footer while routes change", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/");
    const wrapper = mount(App, {
      global: {
        plugins: [router],
        stubs: { RouterView: { template: "<div>route content</div>" } },
      },
    });

    const expectShell = (activePath: string, back: { href: string; label: string } | null = null) => {
      const links = wrapper.findAll('nav[aria-label="主导航"] a');
      expect(links.map((link) => link.text())).toEqual(["新占卜", "历史牌阵", "设置"]);
      expect(links.map((link) => link.attributes("aria-current") ?? "")).toEqual(
        ["/", "/history", "/settings/connection"].map((path) => path === activePath ? "page" : ""),
      );
      expect(wrapper.findAll("footer.app-footer")).toHaveLength(1);
      expect(wrapper.get("footer.app-footer").text()).toBe("© ARCANUM / PRIVATE DESKTOP");
      const backLink = wrapper.find(".page-return");
      if (back) {
        expect(backLink.exists()).toBe(true);
        expect(backLink.attributes("href")).toBe(back.href);
        expect(backLink.text()).toContain(back.label);
      } else {
        expect(backLink.exists()).toBe(false);
      }
      expect(wrapper.text()).toContain("route content");
    };

    expectShell("/");
    await router.push("/history");
    await flushPromises();
    expectShell("/history");
    await router.push("/settings/connection");
    await flushPromises();
    expectShell("/settings/connection");
    await router.push("/reading/draw/draw-1");
    await flushPromises();
    expectShell("/", { href: "/", label: "返回新占卜" });
    await wrapper.get(".page-return").trigger("click");
    await flushPromises();
    expect(router.currentRoute.value.path).toBe("/");
    expectShell("/");

    await router.push("/reading/saved-session");
    await flushPromises();
    expectShell("/history", { href: "/history", label: "返回历史牌阵" });
    await wrapper.get(".page-return").trigger("click");
    await flushPromises();
    expect(router.currentRoute.value.path).toBe("/history");
    expectShell("/history");
    await router.push("/history/saved-session");
    await flushPromises();
    expectShell("/history", { href: "/history", label: "返回历史牌阵" });
    await wrapper.get(".page-return").trigger("click");
    await flushPromises();
    expect(router.currentRoute.value.path).toBe("/history");
    expectShell("/history");

    await router.push("/catalog");
    await flushPromises();
    expectShell("/settings/connection", { href: "/settings/connection", label: "返回设置" });
    await wrapper.get(".page-return").trigger("click");
    await flushPromises();
    expect(router.currentRoute.value.path).toBe("/settings/connection");
    expectShell("/settings/connection");
    wrapper.unmount();
  });
});
