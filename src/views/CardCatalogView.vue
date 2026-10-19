<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import CardZoomDialog, { type CardZoomItem } from "../components/CardZoomDialog.vue";
import { createTarotCatalogStore } from "../tarot/tarot-catalog";
import { tauriTarotCatalogApi } from "../tarot/tauri-tarot-api";

const store = createTarotCatalogStore(tauriTarotCatalogApi);
const selectedGroup = ref("all");
const search = ref("");
const zoomIndex = ref<number | null>(null);
const groups = [
  { id: "all", label: "全部78张" },
  { id: "major", label: "大阿卡纳" },
  { id: "wands", label: "权杖" },
  { id: "cups", label: "圣杯" },
  { id: "swords", label: "宝剑" },
  { id: "pentacles", label: "星币" },
];

const filteredCards = computed(() => {
  const query = search.value.trim().toLocaleLowerCase();
  return store.state.cards.filter((card) => {
    const inGroup = selectedGroup.value === "all"
      || (selectedGroup.value === "major" ? card.arcana === "major" : card.suit === selectedGroup.value);
    const matches = !query || `${card.id} ${card.name_zh} ${card.name_en}`.toLocaleLowerCase().includes(query);
    return inGroup && matches;
  });
});

const zoomCard = computed<CardZoomItem | null>(() => {
  if (zoomIndex.value === null) return null;
  const card = filteredCards.value[zoomIndex.value];
  if (!card) return null;
  const suitLabels = { wands: "权杖", cups: "圣杯", swords: "宝剑", pentacles: "星币" };
  return {
    id: card.id,
    nameZh: card.name_zh,
    nameEn: card.name_en,
    imagePath: card.imagePath,
    slotLabel: card.arcana === "major" ? "大阿卡纳" : suitLabels[card.suit ?? "wands"],
    orientation: "upright",
    keywords: card.upright_keywords,
  };
});
const zoomCounter = computed(() => zoomIndex.value === null ? "" : `${zoomIndex.value + 1} / ${filteredCards.value.length}`);

function openZoom(index: number) {
  zoomIndex.value = index;
}

function navigateZoom(direction: -1 | 1) {
  const count = filteredCards.value.length;
  if (zoomIndex.value === null || count < 2) return;
  zoomIndex.value = (zoomIndex.value + direction + count) % count;
}

onMounted(() => store.load());
</script>

<template>
  <main class="catalog-page">
    <section class="catalog-intro">
      <div class="eyebrow">THE COMPLETE DECK · LOCAL ARCHIVE</div>
      <h1>78张古典牌面</h1>
    </section>

    <section class="catalog-controls" aria-label="筛选牌面">
      <div class="catalog-filters" role="group" aria-label="牌组分类">
        <button v-for="group in groups" :key="group.id" :class="{ selected: selectedGroup === group.id }" @click="selectedGroup = group.id">{{ group.label }}</button>
      </div>
      <label class="catalog-search">
        <span>⌕</span>
        <input v-model="search" type="search" placeholder="按牌名查找" aria-label="按牌名查找" />
      </label>
    </section>

    <p v-if="store.state.loading" class="catalog-feedback" role="status">正在读取本地牌库…</p>
    <div v-else-if="store.state.error" class="catalog-error" role="alert">
      <p>本地牌库暂时无法读取：{{ store.state.error }}</p>
      <button class="button-secondary" @click="store.load()">重新载入</button>
    </div>
    <section v-else class="catalog-grid" aria-label="塔罗牌面目录">
      <article v-for="(card, index) in filteredCards" :key="card.id" class="catalog-card">
        <div class="catalog-art-frame">
          <button class="catalog-art-trigger" type="button" :aria-label="`放大查看${card.name_zh} · ${card.name_en}`" @click="openZoom(index)">
            <img class="catalog-card-image" :src="card.imagePath" :alt="`${card.name_zh} · ${card.name_en}`" loading="eager" decoding="sync" />
          </button>
        </div>
        <div class="catalog-card-heading">
          <span>{{ card.name_zh }}</span>
          <small>{{ card.name_en }}</small>
        </div>
        <div class="catalog-keywords"><span>正位</span>{{ card.upright_keywords.join("、") }}</div>
        <div class="catalog-keywords reversed"><span>逆位</span>{{ card.reversed_keywords.join("、") }}</div>
      </article>
      <p v-if="filteredCards.length === 0" class="catalog-feedback">没有找到符合条件的牌。</p>
    </section>
    <CardZoomDialog :card="zoomCard" :counter="zoomCounter" :can-navigate="filteredCards.length > 1" @close="zoomIndex = null" @previous="navigateZoom(-1)" @next="navigateZoom(1)" />
  </main>
</template>
