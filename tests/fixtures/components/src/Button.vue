<script setup lang="ts">
import { formatLabel } from './shared'

const label = formatLabel('save')
</script>

<template>
  <button>{{ label }}</button>
</template>
