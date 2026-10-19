<script setup lang="ts">
defineProps<{ question: string }>();
</script>

<template>
  <p class="question-summary" :aria-label="question">{{ question }}</p>
</template>
