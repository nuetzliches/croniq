import type { VersionResponse } from '@/api/types'

export interface VersionChipProps {
  version: VersionResponse
}

/** Compact `v<version>` chip with the build sha + time in the tooltip. Shown
 *  on the login screen and in the authenticated topbar so the running build is
 *  always visible. */
export function VersionChip({ version }: VersionChipProps) {
  return (
    <span
      className="tag mono"
      title={`Build ${version.git_sha} · ${version.build_time}`}
      style={{ height: 20, fontSize: 11 }}
    >
      v{version.version}
    </span>
  )
}
