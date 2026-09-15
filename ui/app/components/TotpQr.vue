<script setup lang="ts">
import { ref, watch } from 'vue'
import QRCode from 'qrcode'

/**
 * The enrolment secret as a QR code.
 *
 * The server answers `/totp/setup` with an `otpauth://` URL, which is what
 * every authenticator app expects to scan. The React dashboard rendered it; the
 * rebuild dropped the component but kept the copy telling people to scan —
 * instructions for something the screen did not offer (issue #724).
 *
 * Rendered client-side rather than fetched: the URL contains the shared secret,
 * and handing it to an image service would be handing away the second factor.
 * `qrcode` draws into a canvas locally and nothing leaves the page.
 *
 * The secret stays visible beside this as text. Scanning is the easy path, not
 * the only one — a desktop authenticator, a screen reader, or a camera that
 * will not focus all need the characters.
 */
const props = defineProps<{
  /** The `otpauth://` URL from the setup response. */
  value: string
}>()

const canvas = ref<HTMLCanvasElement | null>(null)
const failed = ref(false)

watch(
  [canvas, () => props.value],
  async ([element, url]) => {
    if (!element || !url) return
    try {
      await QRCode.toCanvas(element, url, {
        width: 180,
        margin: 1,
        // Fixed black on white regardless of theme. A QR code inverted for
        // dark mode is one many scanners refuse, and this is the one image on
        // the page that has to work first time.
        color: { dark: '#000000', light: '#ffffff' },
      })
      failed.value = false
    } catch {
      // Say so rather than leaving a blank square: the secret below is still
      // usable, and an empty box reads as a broken page.
      failed.value = true
    }
  },
  { immediate: true },
)
</script>

<template>
  <div class="flex flex-col gap-2">
    <canvas
      v-show="!failed"
      ref="canvas"
      class="rounded-md bg-white p-2"
      role="img"
      aria-label="QR code containing your authenticator secret"
    />
    <p
      v-if="failed"
      class="text-sm text-muted"
    >
      The QR code could not be drawn. Type the secret below into your
      authenticator instead.
    </p>
  </div>
</template>
