import { afterEach, describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import CardZoomDialog, { type CardZoomItem } from "./CardZoomDialog.vue";

const cards: CardZoomItem[] = [
  { id: "major_00", nameZh: "愚者", nameEn: "The Fool", imagePath: "/fool.webp", slotLabel: "现状", orientation: "upright", keywords: ["启程", "开放"] },
  { id: "major_01", nameZh: "魔术师", nameEn: "The Magician", imagePath: "/magician.webp", slotLabel: "阻碍", orientation: "reversed", keywords: ["迟疑"] },
];

describe("CardZoomDialog", () => {
  let source: HTMLButtonElement | undefined;
  let wrapper: ReturnType<typeof mount> | undefined;

  afterEach(() => {
    wrapper?.unmount();
    source?.remove();
    wrapper = undefined;
    source = undefined;
  });

  it("opens an accessible full-art dialog, shows frozen card details, and restores focus on close", async () => {
    source = document.createElement("button");
    document.body.append(source);
    source.focus();
    wrapper = mount(CardZoomDialog, {
      attachTo: document.body,
      props: { card: cards[0], counter: "1 / 78", canNavigate: true },
    });
    await flushPromises();

    expect(wrapper.get('[role="dialog"]').attributes("aria-modal")).toBe("true");
    expect(wrapper.get("h2").text()).toContain("愚者");
    expect(wrapper.get("img").attributes("src")).toBe("/fool.webp");
    expect(wrapper.text()).toContain("现状");
    expect(wrapper.text()).toContain("正位");
    expect(wrapper.text()).toContain("启程、开放");
    expect(wrapper.text()).toContain("1 / 78");
    expect(document.activeElement).toBe(wrapper.get(".card-zoom-close").element);

    await wrapper.get(".card-zoom-close").trigger("click");
    await wrapper.setProps({ card: null });
    await flushPromises();
    expect(document.activeElement).toBe(source);
  });

  it("maps Escape and arrow keys to close and frozen-card navigation", async () => {
    wrapper = mount(CardZoomDialog, {
      props: { card: cards[0], counter: "1 / 2", canNavigate: true },
    });

    await wrapper.get('[role="dialog"]').trigger("keydown", { key: "ArrowRight" });
    expect(wrapper.emitted("next")).toHaveLength(1);
    await wrapper.get('[role="dialog"]').trigger("keydown", { key: "ArrowLeft" });
    expect(wrapper.emitted("previous")).toHaveLength(1);
    await wrapper.get('[role="dialog"]').trigger("keydown", { key: "Escape" });
    expect(wrapper.emitted("close")).toHaveLength(1);
  });

  it("keeps Tab focus within the modal controls", async () => {
    wrapper = mount(CardZoomDialog, {
      props: { card: cards[0], counter: "1 / 2", canNavigate: true },
      attachTo: document.body,
    });
    await flushPromises();
    const controls = wrapper.findAll("button").map((button) => button.element);

    controls[0].focus();
    await wrapper.get('[role="dialog"]').trigger("keydown", { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(controls.at(-1));
    controls.at(-1)?.focus();
    await wrapper.get('[role="dialog"]').trigger("keydown", { key: "Tab" });
    expect(document.activeElement).toBe(controls[0]);
  });
});
