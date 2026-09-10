<script setup lang="ts">
import { useId } from 'vue'

/**
 * The Croniq orbit mark, ported from `ui/public/icons/mark-mono.svg`.
 *
 * Inline rather than an `<img src>` so it inherits `currentColor` and costs no
 * request. `chip` adds the brand-purple rounded square (the favicon look);
 * without it the glyph renders alone, for use inside containers that already
 * carry a colour.
 *
 * The mask id comes from `useId` rather than being a constant. The React
 * original hardcodes `croniq-mark-gap`, so two marks on one page produce
 * duplicate DOM ids and the second one's mask silently resolves to the first.
 * `docs/vue-migration-plan.md` flagged exactly this when it listed the
 * primitives to port.
 */
withDefaults(
  defineProps<{
    size?: number | string
    /** Spin it — the loading indicator on async actions. */
    spinning?: boolean
    /** Brand-purple chip background, as on the favicon. */
    chip?: boolean
    /** Give the mark an accessible name. Without one it is decorative. */
    title?: string
  }>(),
  { size: 18, spinning: false, chip: false, title: undefined },
)

const maskId = useId()
</script>

<template>
  <svg
    :width="size"
    :height="size"
    viewBox="0 0 100 100"
    xmlns="http://www.w3.org/2000/svg"
    :class="spinning ? 'brand-spin' : undefined"
    :role="title ? 'img' : undefined"
    :aria-label="title"
    :aria-hidden="title ? undefined : true"
  >
    <defs>
      <mask :id="maskId">
        <rect
          width="100"
          height="100"
          fill="white"
        />
        <circle
          cx="76"
          cy="76"
          r="12"
          fill="black"
        />
      </mask>
    </defs>
    <rect
      v-if="chip"
      width="100"
      height="100"
      rx="20"
      fill="#6A54DF"
    />
    <circle
      cx="50"
      cy="50"
      r="34"
      fill="none"
      :stroke="chip ? '#ffffff' : 'currentColor'"
      stroke-width="8"
      :mask="`url(#${maskId})`"
    />
    <circle
      cx="76"
      cy="76"
      r="9"
      :fill="chip ? '#ffffff' : 'currentColor'"
    />
  </svg>
</template>

<style>
/* 0.9 s/turn, matching the React tree's `.brand-spin` in components.css. */
@keyframes brand-spin {
  to {
    transform: rotate(360deg);
  }
}
.brand-spin {
  animation: brand-spin 0.9s linear infinite;
}
</style>
