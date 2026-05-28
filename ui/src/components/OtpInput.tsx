import { useRef } from 'react'

interface OtpInputProps {
  /** Current code as a plain string (≤ `length` digits). */
  value: string
  /** Receives the new code (digits only, ≤ `length`). */
  onChange: (value: string) => void
  length?: number
  autoFocus?: boolean
  disabled?: boolean
  ariaLabel?: string
}

/**
 * Segmented one-time-code input: `length` single-digit boxes bound to one
 * string `value`. Handles auto-advance on entry, backspace (clear + step
 * back), arrow-key navigation, pasting a full code, and password-manager
 * autofill (Bitwarden, 1Password, …) which drops the full code into the
 * first box — multi-digit input is distributed across the boxes from the
 * focused one. Non-digits are ignored, so a user can paste "123 456" or
 * "123-456" and still get "123456".
 */
export function OtpInput({
  value,
  onChange,
  length = 6,
  autoFocus = false,
  disabled = false,
  ariaLabel = 'One-time code',
}: OtpInputProps) {
  const refs = useRef<Array<HTMLInputElement | null>>([])
  const digits = value.split('').slice(0, length)

  const focusBox = (i: number) => {
    const el = refs.current[Math.max(0, Math.min(length - 1, i))]
    el?.focus()
    el?.select()
  }

  // Normalise to digits, cap at `length`, and emit.
  const commit = (next: string): string => {
    const clean = next.replace(/\D/g, '').slice(0, length)
    onChange(clean)
    return clean
  }

  // Current value padded to `length` so positional writes are simple.
  const padded = () => {
    const chars = value.split('')
    while (chars.length < length) chars.push('')
    return chars
  }

  const handleChange = (i: number, raw: string) => {
    const d = raw.replace(/\D/g, '')
    if (!d) return
    const chars = padded()
    if (d.length > 1) {
      // Password-manager autofill (Bitwarden, 1Password, …) drops the full
      // code into the first box. Distribute across boxes like a paste.
      for (let k = 0; k < d.length && i + k < length; k++) {
        chars[i + k] = d[k]
      }
      commit(chars.join(''))
      focusBox(Math.min(i + d.length, length - 1))
      return
    }
    chars[i] = d[0]
    commit(chars.join(''))
    focusBox(i + 1)
  }

  const handleKeyDown = (i: number, e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Backspace') {
      e.preventDefault()
      const chars = padded()
      if (chars[i]) {
        chars[i] = ''
        commit(chars.join(''))
      } else if (i > 0) {
        chars[i - 1] = ''
        commit(chars.join(''))
        focusBox(i - 1)
      }
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault()
      focusBox(i - 1)
    } else if (e.key === 'ArrowRight') {
      e.preventDefault()
      focusBox(i + 1)
    }
  }

  const handlePaste = (i: number, e: React.ClipboardEvent<HTMLInputElement>) => {
    e.preventDefault()
    const pasted = e.clipboardData.getData('text').replace(/\D/g, '')
    if (!pasted) return
    const chars = padded()
    for (let k = 0; k < pasted.length && i + k < length; k++) {
      chars[i + k] = pasted[k]
    }
    commit(chars.join(''))
    focusBox(Math.min(i + pasted.length, length - 1))
  }

  return (
    <div
      className="row"
      role="group"
      aria-label={ariaLabel}
      style={{ gap: 8, justifyContent: 'center' }}
    >
      {Array.from({ length }).map((_, i) => (
        <input
          key={i}
          ref={(el) => {
            refs.current[i] = el
          }}
          className="input mono"
          type="text"
          inputMode="numeric"
          autoComplete={i === 0 ? 'one-time-code' : 'off'}
          name={i === 0 ? 'otp' : undefined}
          pattern="[0-9]*"
          maxLength={i === 0 ? length : 1}
          disabled={disabled}
          autoFocus={autoFocus && i === 0}
          value={digits[i] ?? ''}
          aria-label={`Digit ${i + 1}`}
          onChange={(e) => handleChange(i, e.target.value)}
          onKeyDown={(e) => handleKeyDown(i, e)}
          onPaste={(e) => handlePaste(i, e)}
          onFocus={(e) => e.currentTarget.select()}
          style={{ width: 44, height: 48, textAlign: 'center', fontSize: 20, padding: 0 }}
        />
      ))}
    </div>
  )
}
