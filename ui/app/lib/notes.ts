import type { JobNote, NoteKind } from '~/api/types'

/**
 * Framework-free helpers for operator notes, kept apart from the components so
 * what a badge says can be tested without a component harness (#677).
 */

/** Every kind, in the order the composer offers them, with its wording. */
export const NOTE_KINDS: ReadonlyArray<{
  value: NoteKind
  label: string
  icon: string
  placeholder: string
}> = [
  {
    value: 'ack',
    label: 'Checked',
    icon: 'i-lucide-check',
    placeholder: 'Optional: what you checked, what you found',
  },
  {
    value: 'question',
    label: 'Question',
    icon: 'i-lucide-circle-help',
    placeholder: 'What should a colleague look at?',
  },
  {
    value: 'idea',
    label: 'Idea',
    icon: 'i-lucide-lightbulb',
    placeholder: 'What might fix or prevent this?',
  },
  {
    value: 'note',
    label: 'Note',
    icon: 'i-lucide-message-square',
    placeholder: 'Anything worth knowing next time',
  },
]

/** The icon for a kind. Literal names only — see ADR-0005. */
export function noteIcon(kind: NoteKind): string {
  return NOTE_KINDS.find((entry) => entry.value === kind)?.icon ?? 'i-lucide-message-square'
}

export interface NoteSummary {
  /** Who marked it as checked, newest first, each name once. */
  checkedBy: string[]
  questions: number
  /** Ideas and plain notes. */
  others: number
  total: number
}

/** What a badge says about a set of notes, given newest first. */
export function summarizeNotes(notes: readonly JobNote[]): NoteSummary {
  const checkedBy: string[] = []
  let questions = 0
  let others = 0
  for (const note of notes) {
    if (note.kind === 'ack') {
      if (!checkedBy.includes(note.author_name)) checkedBy.push(note.author_name)
    } else if (note.kind === 'question') {
      questions += 1
    } else {
      others += 1
    }
  }
  return { checkedBy, questions, others, total: notes.length }
}

/** Notes keyed by the run they were written about. Job-level notes are left out. */
export function notesByExecution(notes: readonly JobNote[]): Map<string, JobNote[]> {
  const byRun = new Map<string, JobNote[]>()
  for (const note of notes) {
    if (!note.execution_id) continue
    const list = byRun.get(note.execution_id)
    if (list) list.push(note)
    else byRun.set(note.execution_id, [note])
  }
  return byRun
}
