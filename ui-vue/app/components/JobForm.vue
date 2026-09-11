<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ApiError } from '~/api/client'
import { useCreateJob, useUpdateJob } from '~/api/queries'
import type { JobDefinition } from '~/api/types'

/**
 * Create and edit, in one form.
 *
 * The two differ in exactly one field — the key, which is immutable once the
 * job exists — so two components would be the same file twice.
 *
 * Every optional field is sent as `null` when it is left empty rather than
 * omitted. That is the difference between "inherit the default" and "keep
 * whatever was there", and the server distinguishes them: clearing a timeout
 * has to actually clear it.
 */
const props = defineProps<{ job?: JobDefinition }>()
const open = defineModel<boolean>('open', { required: true })
const emit = defineEmits<{ created: [jobKey: string] }>()

const createJob = useCreateJob()
const updateJob = useUpdateJob()

const editing = computed(() => Boolean(props.job))

const form = ref({
  job_key: '',
  description: '',
  timeout: '',
  max_retries: '',
  tags: '',
  dead_letter_enabled: true,
  dead_letter_retention: '',
  dead_letter_operator_hint: '',
  dead_letter_replay_max_age: '',
})

const error = ref<string | null>(null)

/** Refill on open, so a cancelled edit does not leak into the next one. */
watch(
  () => [open.value, props.job] as const,
  ([isOpen, job]) => {
    if (!isOpen) return
    error.value = null
    form.value = {
      job_key: job?.job_key ?? '',
      description: job?.description ?? '',
      timeout: job?.timeout ?? '',
      max_retries: job?.max_retries == null ? '' : String(job.max_retries),
      tags: (job?.tags ?? []).join(' '),
      dead_letter_enabled: job?.dead_letter_enabled !== false,
      dead_letter_retention: job?.dead_letter_retention ?? '',
      dead_letter_operator_hint: job?.dead_letter_operator_hint ?? '',
      dead_letter_replay_max_age: job?.dead_letter_replay_max_age ?? '',
    }
  },
  { immediate: true },
)

const pending = computed(() => createJob.isPending.value || updateJob.isPending.value)

/** Empty means "no value", which the API spells `null`. */
const orNull = (value: string) => (value.trim() ? value.trim() : null)

async function submit() {
  error.value = null
  const retries = form.value.max_retries.trim()
  const patch = {
    description: orNull(form.value.description),
    timeout: orNull(form.value.timeout),
    max_retries: retries ? Number(retries) : null,
    dead_letter_enabled: form.value.dead_letter_enabled,
    dead_letter_retention: orNull(form.value.dead_letter_retention),
    dead_letter_operator_hint: orNull(form.value.dead_letter_operator_hint),
    dead_letter_replay_max_age: orNull(form.value.dead_letter_replay_max_age),
    tags: form.value.tags.split(/[\s,]+/).filter(Boolean),
  }

  if (retries && Number.isNaN(Number(retries))) {
    error.value = 'Max retries has to be a number, or empty to use the default.'
    return
  }

  try {
    if (props.job) {
      await updateJob.mutateAsync({ job_key: props.job.job_key, ...patch })
    } else {
      const key = form.value.job_key.trim()
      if (!key) {
        error.value = 'A job needs a key.'
        return
      }
      await createJob.mutateAsync({ job_key: key, ...patch })
      emit('created', key)
    }
    open.value = false
  } catch (caught) {
    const body = caught instanceof ApiError ? (caught.body as { message?: string }) : undefined
    error.value = body?.message ?? (caught as Error).message ?? 'The server refused that.'
  }
}
</script>

<template>
  <UModal
    v-model:open="open"
    :title="editing ? `Edit ${job?.job_key}` : 'New job'"
    :description="
      editing
        ? 'Changes apply immediately. The key cannot be changed.'
        : 'A job registered here lives in the API store, not in the Croniqfile.'
    "
  >
    <template #body>
      <form
        class="flex flex-col gap-4"
        @submit.prevent="submit"
      >
        <UAlert
          v-if="error"
          color="warning"
          variant="subtle"
          icon="i-lucide-shield-alert"
          :description="error"
          role="alert"
        />

        <UFormField
          v-if="!editing"
          label="Job key"
          description="Unique, and the name every run and every alert refers to. Convention is namespace:name."
          required
        >
          <UInput
            v-model="form.job_key"
            placeholder="demo:report"
            class="w-full font-mono"
            autofocus
          />
        </UFormField>

        <UFormField label="Description">
          <UInput
            v-model="form.description"
            placeholder="What this job does"
            class="w-full"
          />
        </UFormField>

        <UFormField
          label="Tags"
          description="Space-separated. Used to filter the job list and to route runs to runners."
        >
          <UInput
            v-model="form.tags"
            placeholder="nightly report"
            class="w-full font-mono"
          />
        </UFormField>

        <div class="grid grid-cols-2 gap-4">
          <UFormField
            label="Timeout"
            description="Empty = 5m."
          >
            <UInput
              v-model="form.timeout"
              placeholder="10m"
              class="w-full font-mono"
            />
          </UFormField>
          <UFormField
            label="Max retries"
            description="Empty = server default."
          >
            <UInput
              v-model="form.max_retries"
              placeholder="3"
              class="w-full font-mono"
              inputmode="numeric"
            />
          </UFormField>
        </div>

        <div class="rounded-lg border border-default p-3">
          <USwitch
            v-model="form.dead_letter_enabled"
            label="Keep dead letters"
            description="Off means a run that exhausts its retries is dropped instead of waiting for a decision on /dead-letters."
          />

          <div
            v-if="form.dead_letter_enabled"
            class="mt-3 grid grid-cols-2 gap-3"
          >
            <UFormField
              label="Retention"
              description="Empty = 30d."
            >
              <UInput
                v-model="form.dead_letter_retention"
                placeholder="30d"
                class="w-full font-mono"
              />
            </UFormField>
            <UFormField
              label="Replay max age"
              description="Refuse a replay older than this."
            >
              <UInput
                v-model="form.dead_letter_replay_max_age"
                placeholder="24h"
                class="w-full font-mono"
              />
            </UFormField>
            <UFormField
              class="col-span-2"
              label="Operator hint"
              description="Shown beside a dead letter — what the person deciding replay-or-discard needs to know."
            >
              <UInput
                v-model="form.dead_letter_operator_hint"
                placeholder="Safe to replay; the report is idempotent."
                class="w-full"
              />
            </UFormField>
          </div>
        </div>
      </form>
    </template>

    <template #footer>
      <div class="flex justify-end gap-2">
        <UButton
          color="neutral"
          variant="ghost"
          @click="open = false"
        >
          Cancel
        </UButton>
        <UButton
          :loading="pending"
          @click="submit"
        >
          {{ editing ? 'Save' : 'Create job' }}
        </UButton>
      </div>
    </template>
  </UModal>
</template>
