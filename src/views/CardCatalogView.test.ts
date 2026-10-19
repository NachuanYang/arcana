import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createMemoryHistory } from "vue-router";
import { createAppRouter } from "../router";
import CardCatalogView from "./CardCatalogView.vue";

const { listCards } = vi.hoisted(() => ({ listCards: vi.fn() }));
vi.mock("../tarot/tauri-tarot-api", () => ({ tauriTarotCatalogApi: { listCards } }));

function deck() {
  return Array.from({ length: 78 }, (_, index) => {
    const major = index < 22;
    return {
      id: major ? `major_${String(index).padStart(2, "0")}` : `minor_wands_${String(index - 21).padStart(2, "0")}`,
      arcana: major ? "major" as const : "minor" as const,
      suit: major ? undefined : "wands" as const,
      name_zh: `牌${index}`,
      name_en: `Card ${String(index).padStart(2, "0")}`,
      symbol: "✧",
      upright_keywords: ["清晰"],
      reversed_keywords: ["迟疑"],
      imagePath: `/assets/tarot/rws/card-${index}.webp`,
    };
  });
}

describe("CardCatalogView", () => {
  beforeEach(() => listCards.mockResolvedValue(deck()));

  it("renders all 78 local cards and navigates the filtered catalog from the accessible art viewer", async () => {
    const router = createAppRouter(createMemoryHistory());
    await router.push("/cards");
    const wrapper = mount(CardCatalogView, { attachTo: document.body, global: { plugins: [router] } });
    await flushPromises();

    expect(wrapper.findAll("article.catalog-card")).toHaveLength(78);
    expect(wrapper.text()).not.toContain("Rider–Waite–Smith · Pamela Colman Smith · 同一系列原始扫描，离线查看。");
    expect(wrapper.findAll("img.catalog-card-image").every((image) => image.attributes("loading") === "eager")).toBe(true);
    await wrapper.get(".catalog-filters button:nth-child(2)").trigger("click");
    expect(wrapper.findAll("article.catalog-card")).toHaveLength(22);

    const trigger = wrapper.findAll("button.catalog-art-trigger")[0];
    (trigger.element as HTMLButtonElement).focus();
    await trigger.trigger("click");
    expect(wrapper.get('[role="dialog"]').text()).toContain("牌0");
    expect(wrapper.get('[role="dialog"]').text()).toContain("1 / 22");
    await wrapper.get('[role="dialog"]').trigger("keydown", { key: "ArrowRight" });
    expect(wrapper.get('[role="dialog"] h2').text()).toContain("牌1");
    expect(wrapper.get('[role="dialog"] img').attributes("src")).toBe("/assets/tarot/rws/card-1.webp");
    await wrapper.get('[role="dialog"]').trigger("keydown", { key: "Escape" });
    expect(wrapper.find('[role="dialog"]').exists()).toBe(false);
    expect(document.activeElement).toBe(trigger.element);
    wrapper.unmount();
  });
});
