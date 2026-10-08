import { describe, expect, it } from 'vitest'
import type { JobNote } from '~/api/types'
import { noteExcerpt, noteLabel, noteTone, notesByExecution, summarizeNotes } from './notes'

function note(partial: Partial<JobNote>): JobNote {
  return {
    id: crypto.randomUUID(),
    job_key: 'mail:send',
    execution_id: null,
    kind: 'note',
    body: 'x',
    author_id: 'u1',
    author_name: 'Anna',
    created_at: '2026-10-06T08:00:00Z',
    ...partial,
  }
}

describe('summarizeNotes', () => {
  it('names each person who checked once and counts the rest', () => {
    const summary = summarizeNotes([
      note({ kind: 'ack', author_name: 'Ben' }),
      note({ kind: 'ack', author_name: 'Anna' }),
      note({ kind: 'ack', author_name: 'Ben' }),
      note({ kind: 'question' }),
      note({ kind: 'idea' }),
      note({ kind: 'note' }),
    ])
    expect(summary).toEqual({ checkedBy: ['Ben', 'Anna'], questions: 1, others: 2, total: 6 })
  })

  it('says nothing for no notes', () => {
    expect(summarizeNotes([])).toEqual({ checkedBy: [], questions: 0, others: 0, total: 0 })
  })
})

describe('notesByExecution', () => {
  it('groups by run and leaves job-level notes out', () => {
    const grouped = notesByExecution([
      note({ execution_id: 'run-a' }),
      note({ execution_id: 'run-b' }),
      note({ execution_id: 'run-a' }),
      note({ execution_id: null }),
    ])
    expect(grouped.get('run-a')).toHaveLength(2)
    expect(grouped.get('run-b')).toHaveLength(1)
    expect(grouped.size).toBe(2)
  })
})

describe('one line of a note', () => {
  it('folds a note onto one line', () => {
    expect(noteExcerpt(note({ body: 'disk full\n\n  again?  ' }))).toBe('disk full again?')
  })

  it('says "checked" for an empty check, which has no words of its own', () => {
    expect(noteExcerpt(note({ kind: 'ack', body: '' }))).toBe('checked')
    expect(noteExcerpt(note({ kind: 'ack', body: 'relay restarted' }))).toBe('relay restarted')
  })
})

describe('how a kind is shown', () => {
  it('colours a check as good news and a question as wanting an answer', () => {
    expect(noteTone('ack')).toBe('text-success')
    expect(noteTone('question')).toBe('text-warning')
    expect(noteTone('idea')).toBe('text-muted')
    expect(noteTone('note')).toBe('text-muted')
  })

  it('names a kind as the composer does', () => {
    expect(noteLabel('ack')).toBe('Checked')
    expect(noteLabel('question')).toBe('Question')
  })
})
