import { useState } from 'react'
import { useForm } from 'react-hook-form'
import * as Dialog from '@radix-ui/react-dialog'
import { Plus, Trash2, X, KeyRound, Copy, Check } from 'lucide-react'
import { useApiClients, useCreateApiClient, useDeleteApiClient, useIssueClientToken } from '@/api/hooks'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/card'
import { EmptyState } from '@/components/ui/empty-state'
import { Spinner } from '@/components/ui/spinner'
import type { CreateApiKeyResponse } from '@/api/types'

interface ClientForm { name: string }

function CopyButton({ value }: { value: string }) {
  const [copied, setCopied] = useState(false)
  function copy() {
    navigator.clipboard.writeText(value)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }
  return (
    <button onClick={copy} aria-label="Copy to clipboard" className="text-muted-foreground hover:text-foreground transition-colors">
      {copied ? <Check className="h-3.5 w-3.5 text-status-ok-fg" /> : <Copy className="h-3.5 w-3.5" />}
    </button>
  )
}

export function SettingsPage() {
  const { data: clients, isLoading } = useApiClients()
  const createClient = useCreateApiClient()
  const deleteClient = useDeleteApiClient()
  const issueToken = useIssueClientToken()
  const [open, setOpen] = useState(false)
  const [newKey, setNewKey] = useState<CreateApiKeyResponse | null>(null)

  const { register, handleSubmit, reset, formState: { errors } } = useForm<ClientForm>()

  const inputCls = 'w-full px-3 py-2 border border-border rounded-md text-sm bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary'

  async function onCreate(data: ClientForm) {
    await createClient.mutateAsync({ name: data.name })
    reset()
    setOpen(false)
  }

  async function handleIssueToken(clientId: string) {
    const result = await issueToken.mutateAsync(clientId)
    setNewKey(result)
  }

  return (
    <div className="space-y-6 max-w-2xl">
      {/* API Clients */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <CardTitle className="text-sm font-semibold text-foreground">API Clients</CardTitle>
            <Dialog.Root open={open} onOpenChange={setOpen}>
              <Dialog.Trigger asChild>
                <Button size="sm"><Plus className="h-3.5 w-3.5" />New Client</Button>
              </Dialog.Trigger>
              <Dialog.Portal>
                <Dialog.Overlay className="fixed inset-0 z-40 bg-black/40" />
                <Dialog.Content className="fixed left-1/2 top-1/2 z-50 w-full max-w-sm -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-card p-6 shadow-xl">
                  <div className="flex items-center justify-between mb-4">
                    <Dialog.Title className="text-sm font-semibold">New API Client</Dialog.Title>
                    <Dialog.Close className="text-muted-foreground hover:text-foreground">
                      <X className="h-4 w-4" />
                    </Dialog.Close>
                  </div>
                  <form onSubmit={handleSubmit(onCreate)} className="space-y-3">
                    <div>
                      <input {...register('name', { required: 'Required' })} placeholder="Client name (e.g. my-service)" className={inputCls} />
                      {errors.name && <p className="text-xs text-destructive mt-1">{errors.name.message}</p>}
                    </div>
                    <div className="flex justify-end gap-2">
                      <Dialog.Close asChild><Button variant="secondary" size="sm" type="button">Cancel</Button></Dialog.Close>
                      <Button type="submit" size="sm" disabled={createClient.isPending}>
                        {createClient.isPending ? <Spinner className="h-3.5 w-3.5" /> : 'Create'}
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
                  <p className="text-xs text-muted-foreground font-mono">{c.client_id}</p>
                </div>
                <Button
                  variant="secondary" size="sm"
                  onClick={() => handleIssueToken(c.client_id)}
                  disabled={issueToken.isPending}
                >
                  <KeyRound className="h-3.5 w-3.5" />Issue Key
                </Button>
                <Button
                  variant="ghost" size="sm"
                  onClick={() => deleteClient.mutate(c.client_id)}
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
            <div className="mt-2 grid grid-cols-2 gap-x-4 text-xs">
              <span className="text-muted-foreground">Key ID</span>
              <span className="font-mono">{newKey.key_id}</span>
            </div>
            <Button variant="ghost" size="sm" onClick={() => setNewKey(null)} className="mt-3">Dismiss</Button>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
