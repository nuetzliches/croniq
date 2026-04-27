import { useState } from 'react'
import { useForm } from 'react-hook-form'
import * as Dialog from '@radix-ui/react-dialog'
import { Plus, Pencil, Trash2, X, KeyRound } from 'lucide-react'
import {
  useApiClients,
  useCreateApiClient,
  useUpdateApiClient,
  useDeleteApiClient,
  useIssueClientToken,
} from '@/api/hooks'
import type { ApiClient } from '@/api/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/card'
import { EmptyState } from '@/components/ui/empty-state'
import { Spinner } from '@/components/ui/spinner'
import { useConfirm } from '@/components/ui/confirm-dialog'
import { CopyButton } from '@/components/ui/copy-button'
import type { CreateApiKeyResponse } from '@/api/types'

interface ClientForm { name: string }

// Mirror of `croniq_auth::Scope` (crates/croniq-auth/src/context.rs).
// Keep grouping/ordering aligned with the README's *Scopes* table.
const SCOPE_GROUPS: { label: string; scopes: { value: string; hint?: string }[] }[] = [
  {
    label: 'Admin',
    scopes: [{ value: 'admin', hint: 'Grants every scope below' }],
  },
  {
    label: 'Jobs',
    scopes: [
      { value: 'jobs:read' },
      { value: 'jobs:write' },
      { value: 'jobs:register', hint: 'POST /v1/jobs/register (runner SDK)' },
      { value: 'jobs:trigger', hint: 'POST /v1/trigger (manual fire)' },
    ],
  },
  {
    label: 'Schedules',
    scopes: [
      { value: 'schedules:read' },
      { value: 'schedules:write' },
    ],
  },
  {
    label: 'Calendars',
    scopes: [
      { value: 'calendars:read' },
      { value: 'calendars:write' },
    ],
  },
  {
    label: 'Executions',
    scopes: [{ value: 'executions:read', hint: 'Includes /executions/{id}/logs' }],
  },
  {
    label: 'Dead letters',
    scopes: [
      { value: 'dead-letters:read' },
      { value: 'dead-letters:write', hint: 'Replay + delete' },
    ],
  },
  {
    label: 'Runners',
    scopes: [
      { value: 'runners:read', hint: 'Includes /runners/stream (SSE)' },
      { value: 'runners:write' },
      { value: 'runners:heartbeat' },
    ],
  },
  {
    label: 'Runner pull-protocol',
    scopes: [
      { value: 'work:poll' },
      { value: 'work:ack' },
      { value: 'work:renew' },
      { value: 'work:events' },
    ],
  },
  {
    label: 'Identity / auth management',
    scopes: [
      { value: 'api-clients:admin' },
      { value: 'api-keys:admin' },
    ],
  },
]

export function SettingsPage() {
  const { data: clients, isLoading } = useApiClients()
  const createClient = useCreateApiClient()
  const updateClient = useUpdateApiClient()
  const deleteClient = useDeleteApiClient()
  const issueToken = useIssueClientToken()
  const { confirm, dialog: confirmDialog } = useConfirm()
  const [open, setOpen] = useState(false)
  const [newKey, setNewKey] = useState<CreateApiKeyResponse | null>(null)
  const [scopes, setScopes] = useState<string[]>([])
  const [submitAttempted, setSubmitAttempted] = useState(false)
  // null → create mode, set → edit mode (form is seeded with the
  // client's current name/scopes; submit calls PUT instead of POST).
  const [editingClient, setEditingClient] = useState<ApiClient | null>(null)

  const { register, handleSubmit, reset, formState: { errors } } = useForm<ClientForm>()

  const inputCls = 'w-full px-3 py-2 border border-border rounded-md text-sm bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary'

  function toggleScope(scope: string) {
    setScopes((prev) => (prev.includes(scope) ? prev.filter((s) => s !== scope) : [...prev, scope]))
  }

  function resetDialog() {
    reset({ name: '' })
    setScopes([])
    setSubmitAttempted(false)
    setEditingClient(null)
  }

  function openEdit(client: ApiClient) {
    reset({ name: client.name })
    setScopes(client.scopes)
    setSubmitAttempted(false)
    setEditingClient(client)
    setOpen(true)
  }

  async function onSubmit(data: ClientForm) {
    setSubmitAttempted(true)
    if (scopes.length === 0) return
    if (editingClient) {
      await updateClient.mutateAsync({
        client_id: editingClient.client_id,
        name: data.name,
        scopes,
      })
    } else {
      await createClient.mutateAsync({ name: data.name, scopes })
    }
    resetDialog()
    setOpen(false)
  }

  async function handleIssueToken(client: { client_id: string; name: string }) {
    // Issuing a key is destructive in the sense that it grants long-lived
    // access — make the user confirm so a stray click doesn't silently
    // mint credentials. The reveal card afterwards still requires they
    // copy the key themselves.
    const ok = await confirm({
      title: `Issue API key for ${client.name}?`,
      description:
        'A new API key will be created with the client\'s scopes. The raw key is shown only once — copy it before dismissing.',
      confirmLabel: 'Issue key',
    })
    if (!ok) return
    const result = await issueToken.mutateAsync(client.client_id)
    setNewKey(result)
  }

  async function handleDeleteClient(client: { client_id: string; name: string }) {
    const ok = await confirm({
      title: `Delete client ${client.name}?`,
      description:
        'This revokes every API key bound to the client. Anything authenticating with one of those keys will start receiving 401 immediately.',
      confirmLabel: 'Delete client',
      destructive: true,
    })
    if (ok) deleteClient.mutate(client.client_id)
  }

  return (
    <div className="space-y-6 max-w-2xl">
      {confirmDialog}
      {/* API Clients */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <CardTitle className="text-sm font-semibold text-foreground">API Clients</CardTitle>
            <Dialog.Root open={open} onOpenChange={(v) => { setOpen(v); if (!v) resetDialog() }}>
              <Dialog.Trigger asChild>
                <Button size="sm"><Plus className="h-3.5 w-3.5" />New Client</Button>
              </Dialog.Trigger>
              <Dialog.Portal>
                <Dialog.Overlay className="fixed inset-0 z-40 bg-black/40" />
                <Dialog.Content className="fixed left-1/2 top-1/2 z-50 w-full max-w-md -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-card p-6 shadow-xl max-h-[85vh] overflow-y-auto">
                  <div className="flex items-center justify-between mb-4">
                    <Dialog.Title className="text-sm font-semibold">
                      {editingClient ? `Edit API Client — ${editingClient.name}` : 'New API Client'}
                    </Dialog.Title>
                    <Dialog.Close
                      aria-label="Close dialog"
                      className="text-muted-foreground hover:text-foreground"
                    >
                      <X className="h-4 w-4" />
                    </Dialog.Close>
                  </div>
                  <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
                    <div>
                      <input {...register('name', { required: 'Required' })} placeholder="Client name (e.g. my-service)" className={inputCls} />
                      {errors.name && <p className="text-xs text-destructive mt-1">{errors.name.message}</p>}
                    </div>
                    <div>
                      <div className="flex items-center justify-between mb-2">
                        <label className="text-xs font-medium text-foreground">Scopes</label>
                        {scopes.length > 0 && (
                          <button
                            type="button"
                            onClick={() => setScopes([])}
                            className="text-xs text-muted-foreground hover:text-foreground"
                          >
                            Clear ({scopes.length})
                          </button>
                        )}
                      </div>
                      <div className="space-y-3 border border-border rounded-md p-3 bg-background">
                        {SCOPE_GROUPS.map((group) => (
                          <fieldset key={group.label} className="space-y-1">
                            <legend className="text-xs font-semibold text-muted-foreground mb-1">{group.label}</legend>
                            {/* Single column — hints can be long and a 2-col
                                grid was crashing them into neighbouring
                                checkboxes. The list is short enough that
                                stacking reads fine. */}
                            <div className="space-y-1">
                              {group.scopes.map((s) => {
                                // `admin` is a wildcard that grants every
                                // other scope (see `CallerContext::has_scope`).
                                // When it's selected, show the rest as
                                // checked + disabled so the UI reflects the
                                // implication — and don't pollute the
                                // `scopes` array with redundant entries on
                                // submit.
                                const adminGranted = scopes.includes('admin')
                                const isAdmin = s.value === 'admin'
                                const disabled = adminGranted && !isAdmin
                                const checked = isAdmin
                                  ? adminGranted
                                  : adminGranted || scopes.includes(s.value)
                                return (
                                  <label
                                    key={s.value}
                                    className={`flex items-start gap-2 text-xs select-none ${
                                      disabled ? 'cursor-not-allowed opacity-60' : 'cursor-pointer'
                                    }`}
                                  >
                                    <input
                                      type="checkbox"
                                      checked={checked}
                                      disabled={disabled}
                                      onChange={() => toggleScope(s.value)}
                                      className="h-3.5 w-3.5 mt-0.5 shrink-0 rounded border-border accent-primary disabled:cursor-not-allowed"
                                    />
                                    <span className="min-w-0">
                                      <span className="font-mono">{s.value}</span>
                                      {s.hint && (
                                        <span className="text-muted-foreground font-normal"> — {s.hint}</span>
                                      )}
                                    </span>
                                  </label>
                                )
                              })}
                            </div>
                          </fieldset>
                        ))}
                      </div>
                      {submitAttempted && scopes.length === 0 && (
                        <p className="text-xs text-destructive mt-1">Select at least one scope</p>
                      )}
                    </div>
                    {/* CLI escape-hatch: same client can be seeded at boot
                        with `croniq init --api-key … --scopes …`. Helpful
                        for reproducible bootstrap and air-gapped setups
                        where the dashboard isn't reachable yet. */}
                    <p className="text-xs text-muted-foreground border-t border-border pt-3">
                      Or seed a client at boot:
                      <code className="ml-1 font-mono text-foreground">
                        croniq init --api-key croniq_… --scopes {scopes.length > 0 ? scopes.join(',') : 'jobs:read,executions:read'}
                      </code>
                    </p>
                    <div className="flex justify-end gap-2">
                      <Dialog.Close asChild><Button variant="secondary" size="sm" type="button">Cancel</Button></Dialog.Close>
                      <Button
                        type="submit"
                        size="sm"
                        disabled={createClient.isPending || updateClient.isPending}
                      >
                        {(createClient.isPending || updateClient.isPending) ? (
                          <Spinner className="h-3.5 w-3.5" />
                        ) : editingClient ? 'Save Changes' : 'Create'}
                      </Button>
                    </div>
                  </form>
                </Dialog.Content>
              </Dialog.Portal>
            </Dialog.Root>
          </div>
        </CardHeader>
        <CardContent className="pt-0">
          {isLoading && <div className="flex justify-center py-6"><Spinner className="h-5 w-5" /></div>}
          {!isLoading && clients?.length === 0 && (
            <EmptyState icon={<KeyRound className="h-8 w-8" />} title="No API clients" description="Create a client to generate API keys" />
          )}
          <div className="space-y-2">
            {clients?.map((c) => (
              <div key={c.client_id} className="flex items-center gap-3 p-3 rounded-md border border-border">
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium">{c.name}</span>
                    <Badge variant={c.is_active ? 'ok' : 'neutral'}>{c.is_active ? 'active' : 'inactive'}</Badge>
                  </div>
                  <p className="flex items-center gap-1.5 text-xs text-muted-foreground font-mono">
                    <span>{c.client_id}</span>
                    <CopyButton value={c.client_id} label={`Copy client_id ${c.client_id}`} />
                  </p>
                </div>
                <Button
                  variant="secondary" size="sm"
                  onClick={() => handleIssueToken(c)}
                  disabled={issueToken.isPending}
                >
                  <KeyRound className="h-3.5 w-3.5" />Issue Key
                </Button>
                <Button
                  variant="ghost" size="sm"
                  onClick={() => openEdit(c)}
                  aria-label={`Edit client ${c.name}`}
                  className="h-7 w-7 p-0 text-muted-foreground hover:text-primary"
                >
                  <Pencil className="h-3.5 w-3.5" />
                </Button>
                <Button
                  variant="ghost" size="sm"
                  onClick={() => handleDeleteClient(c)}
                  aria-label={`Delete client ${c.name}`}
                  className="h-7 w-7 p-0 text-muted-foreground hover:text-destructive"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </div>
            ))}
          </div>
        </CardContent>
      </Card>

      {/* New key reveal */}
      {newKey && (
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-semibold text-foreground flex items-center gap-2">
              <KeyRound className="h-4 w-4 text-primary" />
              New API Key — copy now, it won't be shown again
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex items-center gap-2 rounded-md bg-muted p-3 font-mono text-xs">
              <span className="flex-1 break-all">{newKey.raw_key}</span>
              <CopyButton value={newKey.raw_key} />
            </div>
            <div className="mt-2 grid grid-cols-2 gap-x-4 text-xs items-center">
              <span className="text-muted-foreground">Key ID</span>
              <span className="flex items-center gap-1.5 font-mono">
                {newKey.key_id}
                <CopyButton value={newKey.key_id} label="Copy key id" />
              </span>
            </div>
            <Button variant="ghost" size="sm" onClick={() => setNewKey(null)} className="mt-3">Dismiss</Button>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
