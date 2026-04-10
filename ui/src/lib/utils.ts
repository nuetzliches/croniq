import { type ClassValue, clsx } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

export function formatDate(iso: string): string {
  return new Date(iso).toLocaleString()
}

export function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n) + "..." : s
}
