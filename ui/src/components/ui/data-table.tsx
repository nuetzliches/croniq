import { type ReactNode } from 'react'
import { cn } from '@/lib/utils'

interface Column<T> {
  key: string
  header: string
  cell: (row: T) => ReactNode
  className?: string
}

interface DataTableProps<T> {
  columns: Column<T>[]
  data: T[]
  onRowClick?: (row: T) => void
  rowKey: (row: T) => string
  className?: string
}

export function DataTable<T>({ columns, data, onRowClick, rowKey, className }: DataTableProps<T>) {
  return (
    <div className={cn('w-full overflow-x-auto', className)}>
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b border-border">
            {columns.map((col) => (
              <th
                key={col.key}
                className={cn('px-3 py-2.5 text-left text-xs font-medium text-muted-foreground uppercase tracking-wide', col.className)}
              >
                {col.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {data.map((row) => (
            <tr
              key={rowKey(row)}
              onClick={() => onRowClick?.(row)}
              className={cn(
                'border-b border-border last:border-0 transition-colors',
                onRowClick && 'cursor-pointer hover:bg-accent/50'
              )}
            >
              {columns.map((col) => (
                <td key={col.key} className={cn('px-3 py-2.5', col.className)}>
                  {col.cell(row)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
