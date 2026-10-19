<script setup lang="ts">
import { computed } from "vue";
import { RouterLink, useRoute } from "vue-router";

const route = useRoute();
const parentNavigation = computed(() => route.meta.parentNavigation as { to: string; label: string } | undefined);

function isActive(section: string): boolean {
  return route.meta.navSection === section;
}
</script>

<template>
  <div class="app-shell">
    <header class="app-header">
      <RouterLink class="wordmark" to="/" aria-label="秘仪 ARCANUM">
        <span class="sigil" aria-hidden="true">✳</span>
        <span>秘仪 <i>·</i> ARCANUM</span>
      </RouterLink>
      <div class="build-label">TAROT DESKTOP <span>TAIONWC / PAM-A</span></div>
      <nav class="product-header-links" aria-label="主导航">
        <RouterLink to="/" :class="{ 'is-active': isActive('new') }" :aria-current="isActive('new') ? 'page' : undefined">新占卜</RouterLink>
        <RouterLink to="/history" :class="{ 'is-active': isActive('history') }" :aria-current="isActive('history') ? 'page' : undefined">历史牌阵</RouterLink>
        <RouterLink to="/settings/connection" :class="{ 'is-active': isActive('settings') }" :aria-current="isActive('settings') ? 'page' : undefined">设置</RouterLink>
      </nav>
    </header>
    <div class="app-main">
      <div v-if="parentNavigation" class="page-return-row">
        <RouterLink class="page-return" :to="parentNavigation.to"><span aria-hidden="true">←</span>{{ parentNavigation.label }}</RouterLink>
      </div>
      <slot />
    </div>
    <footer class="app-footer">© ARCANUM / PRIVATE DESKTOP</footer>
  </div>
</template>
