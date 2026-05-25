import clsx from 'clsx'

export interface AvatarProps {
  name: string
  size?: 'sm' | 'md' | 'lg'
  className?: string
  title?: string
}

function initials(name: string): string {
  const parts = name
    .trim()
    .split(/[\s@._-]+/)
    .filter(Boolean)
  if (parts.length === 0) return '?'
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase()
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase()
}

export function Avatar({ name, size = 'md', className, title }: AvatarProps) {
  return (
    <span
      className={clsx('avatar', size === 'sm' && 'sm', size === 'lg' && 'lg', className)}
      title={title ?? name}
      aria-hidden
    >
      {initials(name)}
    </span>
  )
}
