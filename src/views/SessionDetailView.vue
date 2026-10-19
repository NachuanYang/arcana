<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { RouterLink, useRoute, useRouter } from "vue-router";
import { authFlow, initializeAuthFlow } from "../auth/auth-flow";
import CardZoomDialog, { type CardZoomItem } from "../components/CardZoomDialog.vue";
import QuestionSummary from "../components/QuestionSummary.vue";
import { captureViewportSnapshot, takeAutoReadViewport, type ViewportSnapshot } from "../router/auto-read-viewport";
import { tarotSessionStore } from "../tarot/tarot-session-flow";
import { readingApi } from "../reading/reading-api";
import { latestCompleteReading, validateReadingResult } from "../reading/reading-view-model";
import { followupHistory, validateFollowupResult } from "../reading/followup-view-model";

const route = useRoute();
const router = useRouter();
const sessionId = computed(() => String(route.params.sessionId ?? ""));
const session = computed(() => tarotSessionStore.state.current);
const slotLabels = { present: "现状", obstacle: "阻碍", tendency: "趋向" };
const stanceLabels = { FAVORABLE: "偏有利", CAUTIOUS: "审慎", UNFAVORABLE: "偏不利", INDETERMINATE: "信号不明" };
const zoomIndex = ref<number | null>(null);
const readingBusy = ref(false);
const readingError = ref("");
const activeRequestId = ref("");
const liveReading = ref<ReturnType<typeof validateReadingResult>>(null);
const followupQuestion = ref("");
const followupBusy = ref(false);
const followupError = ref("");
const activeFollowupRequestId = ref("");
const retryFollowupRequestId = ref("");
const retryFollowupQuestion = ref("");
let restorePromise: Promise<void> = Promise.resolve();
let autoReadStartedSessionId = "";
let viewInitialized = false;
const savedReading = computed(() => session.value ? latestCompleteReading(session.value) : null);
const reading = computed(() => savedReading.value ?? liveReading.value);
const followups = computed(() => session.value ? followupHistory(session.value) : []);
const firstAssistantTurn = computed(() => session.value?.turns
  .filter((turn) => turn.role === "assistant")
  .sort((left, right) => left.turnOrder - right.turnOrder)[0] ?? null);
const hasUnreadableSavedReading = computed(() => firstAssistantTurn.value?.status === "COMPLETE" && !reading.value);
const aiReady = computed(() => Boolean(
  authFlow.state.status.connected
  && authFlow.state.status.planUsageAuthorized
  && authFlow.state.selectedModel,
));
const aiSetupMessage = computed(() => {
  if (!authFlow.state.status.connected || !authFlow.state.status.planUsageAuthorized) {
    return "请先在设置中连接ChatGPT并完成计划用量授权。";
  }
  if (!authFlow.state.selectedModel) return "请先在设置中选择一个账号可用模型。";
  return "";
});
const zoomCard = computed<CardZoomItem | null>(() => {
  const drawn = zoomIndex.value === null ? null : session.value?.cards[zoomIndex.value] ?? null;
  if (!drawn) return null;
  return {
    id: drawn.deckCardId,
    nameZh: drawn.card.name_zh,
    nameEn: drawn.card.name_en,
    imagePath: drawn.card.imagePath,
    slotLabel: slotLabels[drawn.slot],
    orientation: drawn.orientation,
    keywords: drawn.orientation === "reversed" ? drawn.card.reversed_keywords : drawn.card.upright_keywords,
  };
});
const zoomCounter = computed(() => zoomIndex.value === null ? "" : `${zoomIndex.value + 1} / ${session.value?.cards.length ?? 3}`);
const canRetryFollowup = computed(() => Boolean(
  retryFollowupRequestId.value
  && followupQuestion.value.trim() === retryFollowupQuestion.value,
));
const canStartFollowup = computed(() => Boolean(
  session.value
  && reading.value
  && session.value.readingStance
  && session.value.state === "READING_READY"
  && aiReady.value
  && followupQuestion.value.trim().length > 0
  && !followupBusy.value,
));
const canStartReading = computed(() => Boolean(
  session.value
  && ["REVEALED", "READING_FAILED_RETRYABLE", "READING_CANCELLED"].includes(session.value.state)
  && !hasUnreadableSavedReading.value
  && aiReady.value
  && !readingBusy.value,
));

watch(sessionId, (id) => {
  zoomIndex.value = null;
  const previousId = session.value?.id;
  if (previousId && previousId !== id) cancelSessionWork(previousId);
  liveReading.value = null;
  readingError.value = "";
  followupError.value = "";
  followupQuestion.value = "";
  retryFollowupRequestId.value = "";
  retryFollowupQuestion.value = "";
  autoReadStartedSessionId = "";
  restorePromise = restoreSession(id);
}, { immediate: true });

watch([aiReady, () => route.query.autoRead, sessionId], () => {
  void attemptAutoRead();
});

onMounted(async () => {
  await initializeAuthFlow();
  viewInitialized = true;
  await restorePromise;
  await attemptAutoRead();
});

onBeforeUnmount(() => {
  if (session.value) cancelSessionWork(session.value.id);
});

function cancelSessionWork(id: string): void {
  if (readingBusy.value && activeRequestId.value) {
    void readingApi.cancel(id, activeRequestId.value).catch(() => undefined);
  }
  if (followupBusy.value && activeFollowupRequestId.value) {
    void readingApi.cancelFollowup(id, activeFollowupRequestId.value).catch(() => undefined);
  }
}

function openZoom(index: number): void {
  zoomIndex.value = index;
}

function navigateZoom(direction: -1 | 1): void {
  const count = session.value?.cards.length ?? 0;
  if (zoomIndex.value === null || count < 2) return;
  zoomIndex.value = (zoomIndex.value + direction + count) % count;
}

async function restoreSession(id: string): Promise<void> {
  const restored = await tarotSessionStore.open(id);
  if (!restored) return;
  const failed = [...followupHistory(restored)].reverse().find((item) =>
    item.assistantTurn && ["FAILED_RETRYABLE", "CANCELLED"].includes(item.assistantTurn.status));
  if (failed?.assistantTurn?.requestId) {
    followupQuestion.value = failed.question;
    retryFollowupRequestId.value = failed.assistantTurn.requestId;
    retryFollowupQuestion.value = failed.question;
  }
}

async function attemptAutoRead(): Promise<void> {
  if (!viewInitialized || route.query.autoRead !== "1" || !session.value || !aiReady.value || readingBusy.value) return;
  const id = session.value.id;
  if (id !== sessionId.value || autoReadStartedSessionId === id || savedReading.value || hasUnreadableSavedReading.value) return;
  if (!["REVEALED", "READING_FAILED_RETRYABLE", "READING_CANCELLED"].includes(session.value.state)) return;

  autoReadStartedSessionId = id;
  const viewport = takeAutoReadViewport(id) ?? captureConversationViewport();
  await router.replace({ name: "session-detail", params: { sessionId: id } });
  await requestReading(viewport);
}

async function requestReading(viewport = captureConversationViewport()): Promise<void> {
  const current = session.value;
  if (!current || !canStartReading.value) return;
  readingBusy.value = true;
  readingError.value = "";
  activeRequestId.value = crypto.randomUUID();
  try {
    const response = await readingApi.start(
      current.id,
      activeRequestId.value,
      authFlow.state.selectedModel,
      authFlow.state.proxyMode,
    );
    liveReading.value = validateReadingResult(response, current);
    if (!liveReading.value) throw new Error("返回内容与已保存的三张牌不一致，未显示该结论。");
    await tarotSessionStore.open(current.id);
  } catch (error) {
    readingError.value = requestErrorMessage(error, "AI解读未完成，可调整网络或授权后重试。");
    await tarotSessionStore.open(current.id);
  } finally {
    readingBusy.value = false;
    await restoreConversationViewport(viewport);
  }
}

async function requestFollowup(): Promise<void> {
  const current = session.value;
  const question = followupQuestion.value.trim();
  if (!current || !canStartFollowup.value) return;
  const viewport = captureConversationViewport();
  followupBusy.value = true;
  followupError.value = "";
  const requestId = canRetryFollowup.value ? retryFollowupRequestId.value : crypto.randomUUID();
  activeFollowupRequestId.value = requestId;
  try {
    const response = await readingApi.startFollowup(
      current.id,
      requestId,
      question,
      authFlow.state.selectedModel,
      authFlow.state.proxyMode,
    );
    const restored = await tarotSessionStore.open(current.id);
    const userTurn = restored?.turns.find((turn) =>
      turn.id === response.replyToTurnId && turn.requestId === requestId && turn.role === "user");
    if (!restored || !userTurn || !validateFollowupResult(response, restored, userTurn)) {
      throw new Error("追问回复与本地牌阵或本次请求不匹配，已停止显示。");
    }
    followupQuestion.value = "";
    retryFollowupRequestId.value = "";
    retryFollowupQuestion.value = "";
  } catch (error) {
    followupError.value = requestErrorMessage(error, "追问未完成，可调整网络或授权后重试。");
    const restored = await tarotSessionStore.open(current.id);
    const failed = restored && followupHistory(restored).find((item) => item.userTurn.requestId === requestId);
    if (failed?.assistantTurn && ["FAILED_RETRYABLE", "CANCELLED"].includes(failed.assistantTurn.status)) {
      followupQuestion.value = failed.question;
      retryFollowupRequestId.value = requestId;
      retryFollowupQuestion.value = failed.question;
    } else if (!failed) {
      retryFollowupRequestId.value = "";
      retryFollowupQuestion.value = "";
    }
  } finally {
    followupBusy.value = false;
    await restoreConversationViewport(viewport);
  }
}

function captureConversationViewport(): ViewportSnapshot {
  return captureViewportSnapshot();
}

async function restoreConversationViewport(viewport: ViewportSnapshot): Promise<void> {
  await nextTick();
  const top = viewport.wasNearBottom
    ? Math.max(document.documentElement.scrollHeight - window.innerHeight, 0)
    : viewport.top;
  window.scrollTo(0, top);
}

function cardName(cardId: string): string {
  return session.value?.cards.find((card) => card.deckCardId === cardId)?.card.name_zh ?? "牌面不匹配";
}

function requestErrorMessage(error: unknown, fallback: string): string {
  const message = typeof error === "string" ? error : error instanceof Error ? error.message : "";
  const cleaned = message.replace(/[\u0000-\u001f\u007f]/g, " ").trim().slice(0, 500);
  return cleaned || fallback;
}

function formatDate(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : new Intl.DateTimeFormat("zh-CN", { dateStyle: "medium", timeStyle: "short" }).format(date);
}
</script>

<template>
  <main class="session-detail-page">
    <section v-if="tarotSessionStore.state.busy" class="session-load-state" role="status">正在恢复本地牌阵…</section>
    <section v-else-if="tarotSessionStore.state.error || !session" class="session-load-state" role="alert">
      <div class="eyebrow">LOCAL SESSION UNAVAILABLE</div>
      <h1>无法打开这组本地牌阵。</h1>
      <p>{{ tarotSessionStore.state.error || "记录不存在或已被删除。" }}</p>
    </section>

    <section v-else class="session-detail-content">
      <div class="session-detail-layout">
        <section class="session-overview-column" aria-label="本次固定牌阵">
          <div class="eyebrow session-date"><span class="eyebrow-line"></span>{{ formatDate(session.createdAt) }} · FIXED SPREAD</div>
          <QuestionSummary :question="session.question" />
          <section class="revealed-spread" aria-label="本地保存的固定牌阵">
            <article v-for="drawn in session.cards" :key="drawn.slot" class="revealed-card">
              <div class="revealed-slot"><span>0{{ drawn.visualOrder + 1 }}</span>{{ slotLabels[drawn.slot] }}</div>
              <button class="revealed-art-frame zoom-art-trigger" type="button" :aria-label="`放大查看${drawn.card.name_zh} · ${slotLabels[drawn.slot]}位 · ${drawn.orientation === 'reversed' ? '逆位' : '正位'}`" @click="openZoom(drawn.visualOrder)">
                <img :class="{ 'reversed-art': drawn.orientation === 'reversed' }" :src="drawn.card.imagePath" :alt="`${drawn.card.name_zh} · ${drawn.card.name_en}`" />
              </button>
              <div class="revealed-card-name"><strong>{{ drawn.card.name_zh }}</strong><small>{{ drawn.card.name_en }}</small></div>
              <span class="orientation-label" :class="{ reversed: drawn.orientation === 'reversed' }">{{ drawn.orientation === "reversed" ? "逆位" : "正位" }}</span>
              <p class="revealed-keywords">{{ (drawn.orientation === "reversed" ? drawn.card.reversed_keywords : drawn.card.upright_keywords).join("、") }}</p>
            </article>
          </section>
        </section>
        <section class="reading-panel conversation-panel" aria-labelledby="conversation-title">
        <div class="reading-heading">
          <div><div class="eyebrow">SAME SPREAD CONVERSATION</div><h2 id="conversation-title" class="reading-headline">围绕这三张牌继续对话</h2></div>
          <span v-if="reading" class="stance-chip" :class="reading.stance.toLowerCase()">{{ stanceLabels[reading.stance] }}</span>
        </div>
        <div data-testid="conversation-transcript" class="conversation-transcript" aria-label="本地保存的完整对话">
          <article class="conversation-entry conversation-user-entry" data-role="user">
            <div class="conversation-speaker">你 · 原问题</div><p>{{ session.question }}</p>
          </article>
          <article v-if="reading" class="conversation-entry conversation-assistant-entry" data-role="assistant">
            <div class="conversation-speaker">秘仪 · 首轮解读</div>
            <h2 class="reading-headline reading-result-headline">{{ reading.headline }}</h2>
            <p class="reading-synthesis">{{ reading.synthesis }}</p>
            <div class="reading-card-explanations">
              <article v-for="explanation in reading.cards" :key="explanation.slot" class="reading-explanation">
                <div><span>{{ slotLabels[explanation.slot] }}</span><strong>{{ cardName(explanation.cardId) }} · {{ explanation.orientation === "reversed" ? "逆位" : "正位" }}</strong></div>
                <p>{{ explanation.explanation }}</p>
              </article>
            </div>
            <div class="reading-reflection"><p><strong>牌间张力</strong>{{ reading.tension }}</p><p><strong>可以尝试</strong>{{ reading.actionableReflection }}</p><p><strong>解读边界</strong>{{ reading.limitsNote }}</p></div>
          </article>
          <article v-else-if="hasUnreadableSavedReading" class="conversation-entry conversation-assistant-entry" data-role="assistant" role="alert">
            已保存的首轮解读未通过本地牌阵校验，因此停止展示。为避免重写已完成结论，本次不能在此牌阵上重新生成。
          </article>
          <template v-for="item in followups" :key="item.userTurn.id">
            <article class="conversation-entry conversation-user-entry" data-role="user">
              <div class="conversation-speaker">你 · 追问</div><p>{{ item.question }}</p>
            </article>
            <article v-if="item.answer" class="conversation-entry conversation-assistant-entry" data-role="assistant">
              <div class="conversation-speaker">秘仪 · 回复</div>
              <p>{{ item.answer.answer }}</p>
              <div class="followup-citations"><span>牌阵依据：</span><small v-for="citation in item.answer.citedCards" :key="citation.cardId">{{ cardName(citation.cardId) }} · {{ slotLabels[citation.slot] }} · {{ citation.orientation === "reversed" ? "逆位" : "正位" }}</small></div>
            </article>
            <article v-else-if="item.assistantTurn?.status === 'PENDING' || item.assistantTurn?.status === 'STREAMING'" class="conversation-entry conversation-assistant-entry conversation-turn-state" data-role="assistant" role="status">
              追问正在处理中；完成牌面与立场校验后才会显示回复。
            </article>
            <article v-else-if="item.assistantTurn?.status === 'FAILED_RETRYABLE' || item.assistantTurn?.status === 'CANCELLED'" class="conversation-entry conversation-assistant-entry conversation-turn-state" data-role="assistant" role="status">
              这条追问未完成，问题已保留；可在下方重试。
            </article>
            <article v-else class="conversation-entry conversation-assistant-entry conversation-turn-state" data-role="assistant" role="alert">
              本地追问记录未通过牌阵校验，已停止显示。
            </article>
          </template>
        </div>

        <section v-if="!reading && !hasUnreadableSavedReading" class="reading-controls initial-reading-controls" :aria-busy="readingBusy" aria-label="首轮AI解读">
          <h3 class="reading-headline">{{ readingBusy ? "正在解读这组牌…" : "首轮解读尚未完成" }}</h3>
          <div v-if="aiReady" class="reading-controls">
            <div class="reading-actions">
              <button v-if="!readingBusy" class="button-primary" :disabled="!canStartReading" @click="requestReading()">{{ session.state === 'READING_FAILED_RETRYABLE' || session.state === 'READING_CANCELLED' ? "重试解读" : "开始解读" }} <span class="arrow">→</span></button>
            </div>
          </div>
          <div v-else class="reading-auth-prompt"><p>{{ aiSetupMessage }}</p><RouterLink class="button-secondary" to="/settings/connection">前往设置</RouterLink></div>
          <p v-if="readingError" class="feedback feedback-error" role="alert">{{ readingError }}</p>
          <p v-else-if="authFlow.state.error" class="feedback feedback-error" role="alert">{{ authFlow.state.error }}</p>
        </section>
        <section v-else-if="reading" class="reading-controls followup-controls conversation-composer" :aria-busy="followupBusy" aria-label="继续本次对话">
          <label class="field-label" for="followup-question">继续追问</label>
          <textarea id="followup-question" v-model="followupQuestion" maxlength="220" rows="3" :disabled="followupBusy" aria-label="继续追问"></textarea>
          <div class="reading-actions">
            <button class="button-primary" :disabled="!canStartFollowup" @click="requestFollowup()">{{ followupBusy ? "正在处理追问…" : canRetryFollowup ? "重试这条追问" : "发送追问" }} <span v-if="!followupBusy" class="arrow">→</span></button>
            <RouterLink v-if="!aiReady" class="button-secondary" to="/settings/connection">{{ aiSetupMessage }} 前往设置</RouterLink>
          </div>
          <p v-if="followupError" class="feedback feedback-error" role="alert">{{ followupError }}</p>
          <p v-else-if="authFlow.state.error" class="feedback feedback-error" role="alert">{{ authFlow.state.error }}</p>
        </section>
        </section>
      </div>
    </section>

    <CardZoomDialog :card="zoomCard" :counter="zoomCounter" :can-navigate="Boolean(session && session.cards.length > 1)" @close="zoomIndex = null" @previous="navigateZoom(-1)" @next="navigateZoom(1)" />

  </main>
</template>
