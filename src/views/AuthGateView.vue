<script setup lang="ts">
import { onMounted } from "vue";
import { RouterLink } from "vue-router";
import { authFlow, initializeAuthFlow } from "../auth/auth-flow";

const store = authFlow;

function selectModel(event: Event): void {
  store.selectModel((event.target as HTMLSelectElement).value);
}

onMounted(() => initializeAuthFlow());
</script>

<template>
  <main class="workspace">
    <section class="auth-layout">
      <div class="intro-panel">
        <div class="eyebrow"><span class="eyebrow-line"></span>THE READING ROOM</div>
        <h1>让象征，<br /><em>回到你手中。</em></h1>
        <div class="orbit" aria-hidden="true">
          <div class="orbit-ring ring-one"></div>
          <div class="orbit-ring ring-two"></div>
          <div class="orbit-star">✦</div>
          <span class="orbit-dot dot-one"></span>
          <span class="orbit-dot dot-two"></span>
          <span class="orbit-dot dot-three"></span>
        </div>
        <RouterLink class="catalog-entry" to="/catalog">浏览完整牌典 <span>↗</span></RouterLink>
      </div>

      <section class="auth-card" aria-labelledby="connection-title">
        <div class="card-heading">
          <div>
            <div class="eyebrow">ACCOUNT CONNECTION</div>
            <h2 id="connection-title">连接ChatGPT</h2>
          </div>
          <div class="connection-indicator" :class="{ connected: store.state.status.connected && store.state.status.planUsageAuthorized }">
            <span></span>{{ store.state.status.connected ? (store.state.status.planUsageAuthorized ? "已授权" : "权限不足") : "未连接" }}
          </div>
        </div>

        <div class="button-row">
          <button v-if="!store.state.status.connected" class="button-primary" :disabled="store.state.busy" @click="store.signIn()">
            <span v-if="store.state.busy" class="spinner"></span>
            {{ store.state.busy ? "等待系统浏览器授权…" : "使用ChatGPT账号连接" }}
            <span v-if="!store.state.busy" class="arrow">↗</span>
          </button>
          <button v-else class="button-secondary" :disabled="store.state.busy" @click="store.signOut()">断开账号</button>
        </div>

        <div v-if="store.state.status.connected && store.state.status.planUsageAuthorized" class="model-section">
          <div class="section-divider"><span>授权连通性验证</span></div>
          <div class="field-label">网络路线</div>
          <div class="segmented-control" role="group" aria-label="网络路线">
            <button :class="{ selected: store.state.proxyMode === 'system' }" @click="store.setProxyMode('system')">系统代理</button>
            <button :class="{ selected: store.state.proxyMode === 'direct' }" @click="store.setProxyMode('direct')">直连网络</button>
          </div>
          <div class="model-row">
            <label class="field-label" for="model-select">账号模型</label>
            <select id="model-select" :value="store.state.selectedModel" :disabled="store.state.busy || store.state.models.length === 0" @change="selectModel">
              <option value="">先刷新模型目录</option>
              <option v-for="model in store.state.models" :key="model.slug" :value="model.slug">{{ model.displayName }}</option>
            </select>
            <button class="refresh-button" :disabled="store.state.busy" aria-label="刷新模型目录" @click="store.refreshModels()">↻</button>
          </div>
          <button class="button-primary probe-button" :disabled="!store.canInfer.value" @click="store.runInference()">
            开始在线验证 <span class="arrow">→</span>
          </button>
          <div v-if="store.state.lastProbe" class="probe-result"><span class="result-check">✓</span> 收到已完成的真实响应</div>
          <button class="button-text usage-link" @click="store.openUsage()">打开ChatGPT用量管理 ↗</button>
        </div>

        <div v-if="store.state.error" class="feedback feedback-error" role="alert">{{ store.state.error }}</div>
        <div v-else-if="store.state.message" class="feedback" role="status">{{ store.state.message }}</div>

        <div class="card-footer"><span>OPENAI OFFICIAL AUTHORIZATION</span><span>LOCAL FIRST · USER CONTROLLED</span></div>
      </section>
    </section>
  </main>
</template>

<script lang="ts">
export default { name: "ArcanumAuthGate" };
</script>
