import { useEffect, useState } from 'react'
import QRCode from 'qrcode'

/**
 * Render an `otpauth://` URL as a QR code. Produces an inline SVG (no network
 * roundtrip) that scans cleanly on a dark background. Shared by the Settings
 * TOTP setup and the login-time enrolment flow.
 */
export function TotpQr({ value, size = 180 }: { value: string; size?: number }) {
  const [svg, setSvg] = useState<string>('')
  useEffect(() => {
    let cancelled = false
    QRCode.toString(value, {
      type: 'svg',
      errorCorrectionLevel: 'M',
      margin: 1,
      color: { dark: '#0b0b14', light: '#ffffff' },
      width: size,
    })
      .then((s) => {
        if (!cancelled) setSvg(s)
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [value, size])
  return (
    <div
      style={{
        background: '#ffffff',
        padding: 10,
        borderRadius: 'var(--r-2)',
        border: '1px solid var(--border)',
        lineHeight: 0,
        flexShrink: 0,
      }}
      aria-label="TOTP QR code"
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  )
}
