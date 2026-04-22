import { type ButtonHTMLAttributes, forwardRef } from 'react'
import { cn } from '@/lib/utils'

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'secondary' | 'ghost' | 'destructive'
  size?: 'sm' | 'md' | 'lg'
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = 'primary', size = 'md', ...props }, ref) => {
    return (
      <button
        ref={ref}
        className={cn(
          'inline-flex items-center justify-center gap-1.5 rounded-md font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:pointer-events-none disabled:opacity-50',
          {
            'bg-primary text-primary-foreground hover:bg-primary/90': variant === 'primary',
            'bg-accent text-accent-foreground hover:bg-accent/80': variant === 'secondary',
            'hover:bg-accent hover:text-accent-foreground text-muted-foreground': variant === 'ghost',
            'bg-destructive text-white hover:bg-destructive/90': variant === 'destructive',
          },
          {
            'h-7 px-2.5 text-xs': size === 'sm',
            'h-9 px-3.5 text-sm': size === 'md',
            'h-10 px-4 text-sm': size === 'lg',
          },
          className
        )}
        {...props}
      />
    )
  }
)
Button.displayName = 'Button'
