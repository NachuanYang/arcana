<script setup lang="ts">
import { onMounted, ref } from "vue";
import { RouterLink } from "vue-router";
import type { TarotSessionSummary } from "../tarot/tarot-draw-store";
import { tarotSessionStore } from "../tarot/tarot-session-flow";

const pendingDeletion = ref<TarotSessionSummary | null>(null);
const slotLabels = { present: "现状", obstacle: "阻碍", tendency: "趋向" };

onMounted(() => tarotSessionStore.refresh());

function formatDate(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : new Intl.DateTimeFormat("zh-CN", { dateStyle: "medium", timeStyle: "short" }).format(date);
}

async function confirmDelete() {
  if (!pendingDeletion.value) return;
  const deleted = await tarotSessionStore.delete(pendingDeletion.value.id);
  if (deleted) pendingDeletion.value = null;
}
</script>

<template>
  <main class="history-page">
    <section class="history-content">
      <div class="history-heading">
        <div class="eyebrow"><span class="eyebrow-line"></span>LOCAL ARCHIVE</div>
        <h1>历史牌阵</h1>
      </div>

      <p v-if="tarotSessionStore.state.error" class="feedback feedback-error" role="alert">{{ tarotSessionStore.state.error }}</p>
      <div v-if="tarotSessionStore.state.busy && tarotSessionStore.state.sessions.length === 0" class="history-empty" role="status">正在读取本地历史…</div>
      <div v-else-if="tarotSessionStore.state.sessions.length === 0" class="history-empty">
        <div class="empty-sigil">✳</div>
        <h2>还没有保存的牌阵</h2>
        <RouterLink class="button-primary" to="/">开始新占卜 <span class="arrow">→</span></RouterLink>
      </div>
      <section v-else class="history-list" aria-label="已保存的牌阵">
        <article v-for="session in tarotSessionStore.state.sessions" :key="session.id" class="history-item">
          <RouterLink class="history-open" :to="{ name: 'session-detail', params: { sessionId: session.id } }" :aria-label="`回看牌阵：${session.question}`">
            <span class="history-date">{{ formatDate(session.createdAt) }}</span>
            <strong class="history-question">{{ session.question }}</strong>
            <span class="history-cards" aria-label="固定的三张牌">
              <span v-for="drawn in session.cards" :key="drawn.slot" class="history-card-thumb">
                <img :src="drawn.card.imagePath" alt="" />
                <small>{{ slotLabels[drawn.slot] }}</small>
              </span>
            </span>
          </RouterLink>
          <button class="button-text delete-session" type="button" :disabled="tarotSessionStore.state.busy" :aria-label="`删除牌阵：${session.question}`" @click="pendingDeletion = session">删除</button>
        </article>
      </section>
      <button v-if="tarotSessionStore.state.error && tarotSessionStore.state.sessions.length === 0" class="button-secondary history-retry" type="button" @click="tarotSessionStore.refresh()">重试读取</button>
    </section>
    <div v-if="pendingDeletion" class="dialog-backdrop" @click.self="pendingDeletion = null">
      <section class="delete-dialog" role="alertdialog" aria-modal="true" aria-labelledby="delete-title" aria-describedby="delete-description">
        <div class="eyebrow">DELETE LOCAL SESSION</div>
        <h2 id="delete-title">永久删除这组牌阵？</h2>
        <p id="delete-description">“{{ pendingDeletion.question }}”以及它的三张牌面和关联对话都会从本机数据库删除，无法撤销。</p>
        <div class="dialog-actions">
          <button class="button-secondary cancel-delete" type="button" :disabled="tarotSessionStore.state.busy" @click="pendingDeletion = null">保留记录</button>
          <button class="button-danger confirm-delete" type="button" :disabled="tarotSessionStore.state.busy" @click="confirmDelete">{{ tarotSessionStore.state.busy ? "正在删除…" : "确认永久删除" }}</button>
        </div>
      </section>
    </div>
  </main>
</template>
