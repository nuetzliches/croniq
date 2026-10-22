<script setup lang="ts">
import { computed, ref } from 'vue'
import { useAckJob, useClearJobAck, useJobAlertAcks } from '~/api/queries'
import { deadlineFrom } from '~/lib/deadline'
import { formatAbsolute, formatRelative } from '~/lib/format'
import { useActionError } from '~/composables/useActionError'
import { useCanAckAlerts } from '~/composables/useCanAckAlerts'

/**
 * "We know this job is failing, and we are waiting it out."
 *
 * What a rule snooze is not: it quiets one job instead of every job the rule
 * matches, and it needs nobody to remember to lift it. The job's next success
 * ends it and tells the alert channels; its deadline ends it with an alert
 * that the job is still failing. So the panel says both, every time — an
 * acknowledgement that hid how it ends would be the forgotten snooze again.
 *
 * Offered only after a failure: the server refuses one for a job whose latest
 * run succeeded, and says why if this guess is wrong.
 */
const props = defineProps<{
  jobKey: string
  /** When the job last failed, from its stats; `null` when it has not lately. */
  lastFailureAt: string | null
}>()

const { data: acks } = useJobAlertAcks()
const ackJob = useAckJob()
const clearAck = useClearJobAck()
const canAck = useCanAckAlerts()
const { error: actionError, attempt } = useActionError()

const ack = computed(() => (acks.value ?? []).find((a) => a.job_key === props.jobKey) ?? null)

const composing = ref(false)
const note = ref('')
const duration = ref('3d')

function startComposing() {
  actionError.value = null
  // Renewing keeps the reason unless it changed, which is the usual case.
  note.value = ack.value?.note ?? ''
  duration.value = '3d'
  composing.value = true
}

async function submit() {
  const trimmed = note.value.trim()
  if (!trimmed) {
    actionError.value = 'A note is required — whoever wonders why this job is quiet needs to know.'
    return
  }
  const until = deadlineFrom(duration.value)
  if (!until) {
    actionError.value = 'Use a length like 12h or 3d — at most 7d.'
    return
  }
  if (await attempt(() => ackJob.mutateAsync({ jobKey: props.jobKey, note: trimmed, until }))) {
    composing.value = false
  }
}

function resume() {
  void attempt(() => clearAck.mutateAsync(props.jobKey))
}

const heldBack = computed(() => {
  const n = ack.value?.suppressed_count ?? 0
  return n === 0 ? 'No failed run held back yet.' : `${n} failed run${n === 1 ? '' : 's'} held back.`
})
</script>

<template>
  <div
    v-if="ack || (lastFailureAt && canAck)"
    class="mb-4"
  >
    <UAlert
      v-if="actionError"
      class="mb-3"
      color="warning"
      variant="subtle"
      icon="i-lucide-shield-alert"
      title="Refused"
      :description="actionError"
      role="alert"
      close
      @update:open="actionError = null"
    />

    <UAlert
      v-if="ack"
      color="info"
      variant="subtle"
      icon="i-lucide-bell-off"
      :title="`Alerts held back — acknowledged by ${ack.acked_by_name} ${formatRelative(ack.acked_at)}`"
    >
      <template #description>
        <p class="break-words">
          {{ ack.note }}
        </p>
        <p class="mt-1">
          {{ heldBack }} Ends when the job next succeeds, which sends a recovery — or at
          {{ formatAbsolute(ack.expires_at) }}, with an alert if it is still failing.
        </p>
        <p
          v-if="ack.last_error"
          v-tooltip="ack.last_error"
          class="mt-1 truncate font-mono text-xs"
        >
          Last: {{ ack.last_error }}
        </p>
      </template>
      <template
        v-if="canAck"
        #actions
      >
        <UButton
          size="xs"
          color="neutral"
          variant="subtle"
          icon="i-lucide-calendar-plus"
          @click="startComposing"
        >
          Renew
        </UButton>
        <UButton
          size="xs"
          color="neutral"
          variant="ghost"
          icon="i-lucide-bell-ring"
          :loading="clearAck.isPending.value"
          @click="resume"
        >
          Resume alerts
        </UButton>
      </template>
    </UAlert>

    <UButton
      v-else-if="!composing"
      v-tooltip="'Hold back this job\'s alerts while you wait out a failure you cannot fix — an outage upstream, say. Its next success ends it.'"
      size="xs"
      color="neutral"
      variant="subtle"
      icon="i-lucide-bell-off"
      @click="startComposing"
    >
      Acknowledge failure
    </UButton>

    <div
      v-if="composing"
      class="mt-3 flex flex-col gap-3 rounded-lg border border-default p-3"
    >
      <p class="cq-label">
        {{ ack ? 'Renew the acknowledgement' : 'Acknowledge this failure' }}
      </p>
      <UFormField
        label="At most for"
        description="It ends earlier, with a recovery, the moment the job succeeds. If it runs out first, the channels hear that the job is still failing. 7d at most."
      >
        <UInput
          v-model="duration"
          class="w-32 font-mono"
          placeholder="3d"
          aria-label="Longest the acknowledgement lasts"
        />
      </UFormField>
      <UFormField
        label="Note"
        description="Required. Shown here and in the recovery, so say what you are waiting for."
        required
      >
        <UInput
          v-model="note"
          class="w-full"
          placeholder="Payment provider outage, status page says ETA 18:00 — ticket OPS-512"
          aria-label="Reason for the acknowledgement"
        />
      </UFormField>
      <div class="flex justify-end gap-2">
        <UButton
          color="neutral"
          variant="ghost"
          size="xs"
          @click="composing = false"
        >
          Cancel
        </UButton>
        <UButton
          size="xs"
          :loading="ackJob.isPending.value"
          @click="submit"
        >
          Acknowledge
        </UButton>
      </div>
    </div>
  </div>
</template>
