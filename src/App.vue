<script setup lang="ts">
import { onErrorCaptured, ref } from "vue";
import AppShell from "./components/AppShell.vue";

const rendererFailed = ref(false);

onErrorCaptured(() => {
  rendererFailed.value = true;
  return false;
});

function recoverRenderer() {
  rendererFailed.value = false;
  window.location.hash = "/";
  window.location.reload();
}
</script>

<template>
  <main v-if="rendererFailed" class="error-boundary" role="alert">
    <span class="eyebrow">ARCANUM / RECOVERY</span>
    <h1>界面暂时没有响应。</h1>
    <p>重新加载桌面界面即可恢复操作。</p>
    <button class="button-primary" @click="recoverRenderer">重新加载界面</button>
  </main>
  <AppShell v-else>
    <RouterView />
  </AppShell>
</template>
