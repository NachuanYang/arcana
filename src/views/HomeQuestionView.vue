<script setup lang="ts">
import { computed, ref } from "vue";
import { useRouter } from "vue-router";
import { effectiveQuestionLength } from "../tarot/tarot-draw-store";
import { tarotDrawStore } from "../tarot/tarot-draw-flow";

const router = useRouter();
const question = ref("");
const attemptedSubmit = ref(false);
const questionLength = computed(() => effectiveQuestionLength(question.value));
const questionValid = computed(() => questionLength.value >= 5 && questionLength.value <= 220);
const validationMessage = computed(() => {
  if (!attemptedSubmit.value || questionValid.value) return "";
  if (questionLength.value < 5) return "问题需为5至220个有效字符。";
  if (questionLength.value > 220) return "问题需为5至220个有效字符。";
  return "";
});

async function startReading() {
  attemptedSubmit.value = true;
  if (!questionValid.value) return;
  const offer = await tarotDrawStore.start(question.value);
  if (offer) await router.push({ name: "draw-view", params: { drawId: offer.drawId } });
}
</script>

<template>
  <main class="question-page">
    <section class="question-layout">
      <div class="question-intro">
        <div class="eyebrow"><span class="eyebrow-line"></span>ONE QUESTION · THREE CARDS</div>
        <h1>把你的问题，<br /><em>交给三张牌。</em></h1>
      </div>

      <section class="question-card" aria-labelledby="question-title">
        <div class="eyebrow">BEGIN A READING</div>
        <h2 id="question-title">此刻，你最想看清什么？</h2>
        <form class="question-form" @submit.prevent="startReading">
          <textarea
            id="question-input"
            v-model="question"
            aria-label="占卜问题"
            rows="5"
            :disabled="tarotDrawStore.state.busy"
          ></textarea>
          <p v-if="validationMessage" class="question-validation" role="alert">{{ validationMessage }}</p>
          <p v-if="tarotDrawStore.state.error" class="feedback feedback-error" role="alert">{{ tarotDrawStore.state.error }}</p>
          <button class="button-primary question-submit" type="submit" :disabled="tarotDrawStore.state.busy">
            <span v-if="tarotDrawStore.state.busy" class="spinner"></span>
            {{ tarotDrawStore.state.busy ? "准备牌阵…" : "开始抽牌" }}
            <span v-if="!tarotDrawStore.state.busy" class="arrow">→</span>
          </button>
        </form>
      </section>
    </section>
  </main>
</template>
