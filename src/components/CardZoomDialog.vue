<script setup lang="ts">
import { computed, nextTick, ref, useId, watch } from "vue";

export type CardZoomItem = {
  id: string;
  nameZh: string;
  nameEn: string;
  imagePath: string;
  slotLabel: string;
  orientation: "upright" | "reversed";
  keywords: string[];
};

const props = withDefaults(defineProps<{
  card: CardZoomItem | null;
  counter?: string;
  canNavigate?: boolean;
}>(), { counter: "", canNavigate: false });

const emit = defineEmits<{
  close: [];
  previous: [];
  next: [];
}>();

const closeButton = ref<HTMLButtonElement | null>(null);
const returnFocusTarget = ref<HTMLElement | null>(null);
const dialogTitleId = `card-zoom-title-${useId()}`;
const orientationText = computed(() => props.card?.orientation === "reversed" ? "逆位" : "正位");

watch(() => props.card !== null, async (open) => {
  if (open) {
    returnFocusTarget.value = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    await nextTick();
    closeButton.value?.focus();
    return;
  }
  const target = returnFocusTarget.value;
  await nextTick();
  if (target?.isConnected) target.focus();
}, { flush: "post", immediate: true });

function handleKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.preventDefault();
    emit("close");
    return;
  }
  if (props.canNavigate && event.key === "ArrowLeft") {
    event.preventDefault();
    emit("previous");
    return;
  }
  if (props.canNavigate && event.key === "ArrowRight") {
    event.preventDefault();
    emit("next");
    return;
  }
  if (event.key !== "Tab") return;
  const dialog = event.currentTarget as HTMLElement;
  const controls = [...dialog.querySelectorAll<HTMLElement>(
    'button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
  )];
  if (controls.length === 0) {
    event.preventDefault();
    dialog.focus();
    return;
  }
  const first = controls[0];
  const last = controls[controls.length - 1];
  const active = document.activeElement;
  if (event.shiftKey && (active === first || active === dialog)) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && (active === last || active === dialog)) {
    event.preventDefault();
    first.focus();
  }
}
</script>

<template>
  <div v-if="card" class="card-zoom-backdrop" @click.self="emit('close')">
    <section
      class="card-zoom-dialog"
      role="dialog"
      aria-modal="true"
      :aria-labelledby="dialogTitleId"
      tabindex="-1"
      @keydown="handleKeydown"
    >
      <button ref="closeButton" class="card-zoom-close" type="button" aria-label="关闭牌面查看器" @click="emit('close')">×</button>
      <div class="card-zoom-art" :aria-label="`${card.nameZh}牌面原图`">
        <img :class="{ 'reversed-art': card.orientation === 'reversed' }" :src="card.imagePath" :alt="`${card.nameZh} · ${card.nameEn}`" />
      </div>
      <div class="card-zoom-details">
        <p class="eyebrow">RIDER–WAITE–SMITH · PAM-A</p>
        <p class="card-zoom-position">{{ card.slotLabel }} <span>·</span> {{ orientationText }}</p>
        <h2 :id="dialogTitleId">{{ card.nameZh }}</h2>
        <p class="card-zoom-english">{{ card.nameEn }}</p>
        <p class="card-zoom-keywords">{{ card.keywords.join("、") }}</p>
        <div v-if="counter" class="card-zoom-counter" aria-live="polite">{{ counter }}</div>
        <div v-if="canNavigate" class="card-zoom-navigation" aria-label="切换牌面">
          <button type="button" aria-label="上一张牌" @click="emit('previous')">← 上一张</button>
          <button type="button" aria-label="下一张牌" @click="emit('next')">下一张 →</button>
        </div>
        <p class="card-zoom-hint">使用左右方向键切换，按 Esc 关闭</p>
      </div>
    </section>
  </div>
</template>
