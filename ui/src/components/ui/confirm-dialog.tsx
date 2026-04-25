import { type ReactNode, useState, useCallback } from 'react'
import * as Dialog from '@radix-ui/react-dialog'
import { AlertTriangle } from 'lucide-react'
import { Button } from './button'

interface ConfirmDialogOptions {
  title: string
  description: ReactNode
  confirmLabel?: string
  cancelLabel?: string
  destructive?: boolean
}

interface PendingConfirm extends ConfirmDialogOptions {
  resolve: (ok: boolean) => void
}

/// Imperative `useConfirm()` hook — call `await confirm({...})` from any
/// handler and you get a promise that resolves to `true` if the user
/// clicks confirm, `false` otherwise. The dialog itself is rendered
/// once via the returned element, which the component should mount
/// somewhere in the tree (typically next to the action that uses it).
///
/// Built on @radix-ui/react-dialog (already in use elsewhere) to avoid
/// pulling in @radix-ui/react-alert-dialog as a new dep — the only
/// real difference is the role, which we set explicitly below.
export function useConfirm(): {
  confirm: (opts: ConfirmDialogOptions) => Promise<boolean>
  dialog: ReactNode
} {
  const [pending, setPending] = useState<PendingConfirm | null>(null)

  const confirm = useCallback((opts: ConfirmDialogOptions) => {
    return new Promise<boolean>((resolve) => setPending({ ...opts, resolve }))
  }, [])

  function close(ok: boolean) {
    if (pending) {
      pending.resolve(ok)
      setPending(null)
    }
  }

  const dialog = (
    <Dialog.Root open={pending !== null} onOpenChange={(o) => !o && close(false)}>
      {pending && (
        <Dialog.Portal>
          <Dialog.Overlay className="fixed inset-0 z-40 bg-black/40" />
          <Dialog.Content
            // `role="alertdialog"` so screen readers announce the dialog
            // as critical and don't auto-skip past it.
            role="alertdialog"
            className="fixed left-1/2 top-1/2 z-50 w-full max-w-sm -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-card p-5 shadow-xl"
          >
            <div className="flex gap-3">
              {pending.destructive && (
                <AlertTriangle className="h-5 w-5 shrink-0 text-destructive" />
              )}
              <div className="flex-1">
                <Dialog.Title className="text-sm font-semibold">{pending.title}</Dialog.Title>
                <Dialog.Description className="mt-1 text-xs text-muted-foreground whitespace-pre-wrap">
                  {pending.description}
                </Dialog.Description>
              </div>
            </div>
            <div className="mt-4 flex justify-end gap-2">
              <Button variant="secondary" size="sm" onClick={() => close(false)}>
                {pending.cancelLabel ?? 'Cancel'}
              </Button>
              <Button
                variant={pending.destructive ? 'destructive' : 'primary'}
                size="sm"
                onClick={() => close(true)}
                autoFocus
              >
                {pending.confirmLabel ?? 'Confirm'}
              </Button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      )}
    </Dialog.Root>
  )

  return { confirm, dialog }
}
