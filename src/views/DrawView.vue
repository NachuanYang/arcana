<script setup lang="ts">
import { computed } from "vue";
import { RouterLink, useRoute, useRouter } from "vue-router";
import QuestionSummary from "../components/QuestionSummary.vue";
import { rememberAutoReadViewport } from "../router/auto-read-viewport";
import { tarotDrawStore } from "../tarot/tarot-draw-flow";

const route = useRoute();
const router = useRouter();
const drawId = computed(() => String(route.params.drawId ?? ""));
const offerMatchesRoute = computed(() => tarotDrawStore.state.offer?.drawId === drawId.value);
const selectedCount = computed(() => tarotDrawStore.state.selectedPositions.length);
const revealed = computed(() => tarotDrawStore.state.revealed);
const positions = computed(() => tarotDrawStore.state.offer?.backs.map(({ position }) => position) ?? []);
const slotLabels = { present: "现状", obstacle: "阻碍", tendency: "趋向" };

function isSelected(position: number) {
  return tarotDrawStore.state.selectedPositions.includes(position);
}

async function revealDraw() {
  const snapshot = await tarotDrawStore.reveal();
  if (snapshot) {
    rememberAutoReadViewport(snapshot.drawId);
    await router.replace({
      name: "session-detail",
      params: { sessionId: snapshot.drawId },
      query: { autoRead: "1" },
    });
  }
}
</script>

<template>
  <main class="draw-page">
    <section v-if="!offerMatchesRoute" class="draw-expired" role="alert">
      <div class="eyebrow">DRAW SESSION UNAVAILABLE</div>
      <h1>这组临时牌阵已失效。</h1>
      <p>本地牌阵尚未保存。重新开始会创建新的问题与牌阵。</p>
      <RouterLink class="button-primary" to="/">重新开始</RouterLink>
    </section>

    <template v-else-if="tarotDrawStore.state.offer">
      <section v-if="!revealed" class="draw-content">
        <div class="draw-intro">
          <QuestionSummary :question="tarotDrawStore.state.offer.question" />
        </div>

        <div class="draw-toolbar">
          <div class="selection-status" role="status" aria-live="polite">已选择 <strong>{{ selectedCount }}</strong> / 3</div>
        </div>

        <section class="card-back-grid" role="group" aria-label="十二张可选择的牌背">
          <button
            v-for="position in positions"
            :key="position"
            class="card-back"
            :class="{ selected: isSelected(position) }"
            type="button"
            :aria-label="`第${position + 1}张牌背，${isSelected(position) ? '已选' : '未选'}`"
            :aria-pressed="isSelected(position)"
            :disabled="tarotDrawStore.state.busy || (selectedCount >= 3 && !isSelected(position))"
            @click="tarotDrawStore.togglePosition(position)"
          >
            <img :src="tarotDrawStore.state.offer.backImagePath" alt="" aria-hidden="true" />
            <span class="back-position" aria-hidden="true">{{ String(position + 1).padStart(2, "0") }}</span>
            <span v-if="isSelected(position)" class="back-check" aria-hidden="true">✓</span>
          </button>
        </section>

        <div class="draw-actions">
          <button class="button-primary reveal-action centered-action" type="button" :disabled="!tarotDrawStore.canReveal.value" @click="revealDraw">
            {{ tarotDrawStore.state.busy ? "正在固定牌阵…" : "确认揭牌" }} <span class="arrow">→</span>
          </button>
        </div>
        <p v-if="tarotDrawStore.state.error" class="feedback feedback-error draw-error" role="alert">{{ tarotDrawStore.state.error }}</p>
      </section>

      <section v-else class="reveal-content">
        <div class="eyebrow"><span class="eyebrow-line"></span>THE SPREAD IS FIXED</div>
        <h1>三张牌，已经为你展开。</h1>
        <QuestionSummary :question="revealed.question" />
        <section class="revealed-spread" aria-label="已揭开的固定牌阵">
          <article v-for="drawn in revealed.cards" :key="drawn.slot" class="revealed-card">
            <div class="revealed-slot"><span>0{{ drawn.visualOrder + 1 }}</span>{{ slotLabels[drawn.slot] }}</div>
            <div class="revealed-art-frame">
              <img :class="{ 'reversed-art': drawn.orientation === 'reversed' }" :src="drawn.card.imagePath" :alt="`${drawn.card.name_zh} · ${drawn.card.name_en}`" />
            </div>
            <div class="revealed-card-name">
              <strong>{{ drawn.card.name_zh }}</strong>
              <small>{{ drawn.card.name_en }}</small>
            </div>
            <span class="orientation-label" :class="{ reversed: drawn.orientation === 'reversed' }">{{ drawn.orientation === "reversed" ? "逆位" : "正位" }}</span>
            <p class="revealed-keywords">{{ (drawn.orientation === "reversed" ? drawn.card.reversed_keywords : drawn.card.upright_keywords).join("、") }}</p>
          </article>
        </section>
        <p class="fixed-note" role="status">这组三张牌及方向已固定。重新请求解读时不会重新洗牌或更换牌面。</p>
        <RouterLink class="button-primary new-question-link" to="/">开始一个新问题</RouterLink>
        <p v-if="tarotDrawStore.state.error" class="feedback feedback-error draw-error" role="alert">{{ tarotDrawStore.state.error }}</p>
      </section>
    </template>
  </main>
</template>
