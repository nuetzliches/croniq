# Croniq UI Tailwind Theme & Typography

Theme blueprint for the Angular 21 + Tailwind admin UI. Use these tokens in `tailwind.config.ts`, Storybook stories, and CSS variables scoped by data-theme attributes.

## Goals

- Preserve the telemetry-first, console-inspired look described in `angular-ui-concept.md`.
- Keep semantic tokens stable so Storybook snapshots and E2E tests can rely on deterministic colors/spacing.
- Align with Angular's official Tailwind integration guidance at [https://next.angular.dev/guide/tailwind](https://next.angular.dev/guide/tailwind) and the AI best practices in [https://next.angular.dev/ai/develop-with-ai](https://next.angular.dev/ai/develop-with-ai) when generating code.

## Typography

| Token          | Font stack                                            | Usage                                         |
| -------------- | ----------------------------------------------------- | --------------------------------------------- |
| `font-display` | `"Space Grotesk", "Segoe UI", system-ui, sans-serif`  | Headings, metric callouts, navigation labels. |
| `font-body`    | `"Inter", "Segoe UI", system-ui, sans-serif`          | Body copy, form labels, tables.               |
| `font-mono`    | `"IBM Plex Mono", "SFMono-Regular", Menlo, monospace` | Metrics, timestamps, code/policy diffs.       |

Weights: 500 for display headings, 400 for body, 500 mono for emphasis. Line-height ratio 1.3 for display, 1.5 for body.

## Color Tokens

Define CSS variables on `:root[data-theme="ops-light"]` and `:root[data-theme="ops-dark"]`. Tailwind reads them via `rgb(var(--cq-surface) / <alpha-value>)` style utilities.

### Light Theme (`ops-light`)

| Token                | Value (Hex) | Usage                  |
| -------------------- | ----------- | ---------------------- |
| `--cq-surface`       | `#F8FAFC`   | Page background.       |
| `--cq-surface-alt`   | `#EEF2F7`   | Cards, tables.         |
| `--cq-border`        | `#CBD2DF`   | Dividers, grid lines.  |
| `--cq-text`          | `#1A1F2B`   | Primary text.          |
| `--cq-text-muted`    | `#4B5568`   | Secondary text.        |
| `--cq-accent`        | `#00B1D2`   | Primary buttons, KPIs. |
| `--cq-accent-strong` | `#0084A3`   | Hover/active.          |
| `--cq-danger`        | `#F05252`   | Alerts, failed jobs.   |
| `--cq-warning`       | `#F4B740`   | Misfire warnings.      |
| `--cq-success`       | `#1FAD66`   | Healthy queues.        |
| `--cq-graph-1`       | `#1D4ED8`   | Sparklines (queues).   |
| `--cq-graph-2`       | `#DD2590`   | Sparklines (policies). |

### Dark Theme (`ops-dark`)

| Token                | Value (Hex) | Usage |
| -------------------- | ----------- | ----- |
| `--cq-surface`       | `#0F172A`   |
| `--cq-surface-alt`   | `#1E2A3F`   |
| `--cq-border`        | `#27344D`   |
| `--cq-text`          | `#F8FAFC`   |
| `--cq-text-muted`    | `#94A3B8`   |
| `--cq-accent`        | `#27D2FF`   |
| `--cq-accent-strong` | `#14B8F9`   |
| `--cq-danger`        | `#FB7181`   |
| `--cq-warning`       | `#FACC15`   |
| `--cq-success`       | `#34D399`   |
| `--cq-graph-1`       | `#93C5FD`   |
| `--cq-graph-2`       | `#F9A8D4`   |

## Spacing & Layout

- Base grid: 8px units (`spacing: {0:0px, 1:4px, 2:8px, 3:12px, 4:16px, ...}`) to support dense operator layouts.
- Container widths: 1440px max with 24px gutters.
- Card padding: `px-5 py-4` in Tailwind terms, adjusted down to `px-4 py-3` for drawer content.

## Elevation & Borders

| Token            | Value                               | Notes                     |
| ---------------- | ----------------------------------- | ------------------------- |
| `--cq-radius-sm` | `6px`                               | Buttons, pills.           |
| `--cq-radius-lg` | `12px`                              | Cards, panels.            |
| `shadow-shell`   | `0 8px 30px rgba(15, 23, 42, 0.35)` | Global shell drop shadow. |
| `shadow-card`    | `0 4px 15px rgba(15, 23, 42, 0.18)` | Dashboard cards.          |

## Motion

- `motion-fast`: 120ms cubic-bezier(0.4, 0, 0.2, 1) for hover/press.
- `motion-medium`: 220ms same curve for panel slides and drawers.
- `motion-emphasis`: 320ms cubic-bezier(0.25, 0.8, 0.25, 1) for command palette + overlays.
- Apply reduced-motion media queries to disable sparkline animations if requested.

## Tailwind Config Snippet

```ts
// tailwind.config.ts
import { defineConfig } from "tailwindcss/resolveConfig";

export default defineConfig({
  darkMode: ["class", "[data-theme='ops-dark']"],
  content: ["./apps/**/*.{html,ts}", "./libs/**/*.{html,ts}"],
  theme: {
    fontFamily: {
      display: ['"Space Grotesk"', '"Segoe UI"', "sans-serif"],
      body: ['"Inter"', '"Segoe UI"', "sans-serif"],
      mono: ['"IBM Plex Mono"', '"SFMono-Regular"', "monospace"],
    },
    extend: {
      colors: {
        surface: "rgb(var(--cq-surface) / <alpha-value>)",
        "surface-alt": "rgb(var(--cq-surface-alt) / <alpha-value>)",
        border: "rgb(var(--cq-border) / <alpha-value>)",
        text: "rgb(var(--cq-text) / <alpha-value>)",
        "text-muted": "rgb(var(--cq-text-muted) / <alpha-value>)",
        accent: "rgb(var(--cq-accent) / <alpha-value>)",
        danger: "rgb(var(--cq-danger) / <alpha-value>)",
        warning: "rgb(var(--cq-warning) / <alpha-value>)",
        success: "rgb(var(--cq-success) / <alpha-value>)",
      },
      spacing: {
        1: "4px",
        2: "8px",
        3: "12px",
        4: "16px",
        6: "24px",
        8: "32px",
        10: "40px",
        12: "48px",
      },
      boxShadow: {
        shell: "0 8px 30px rgba(15, 23, 42, 0.35)",
        card: "0 4px 15px rgba(15, 23, 42, 0.18)",
      },
      transitionTimingFunction: {
        fast: "cubic-bezier(0.4, 0, 0.2, 1)",
        emphasis: "cubic-bezier(0.25, 0.8, 0.25, 1)",
      },
      transitionDuration: {
        fast: "120ms",
        medium: "220ms",
        emphasis: "320ms",
      },
    },
  },
  plugins: [],
});
```

## Approval Workflow

1. Export this token table to FigJam/Figma for visual review.
2. Confirm accessibility contrast (WCAG AA) for primary text vs surfaces across both themes.
3. Sign-off stakeholders: UI lead + Observability owner. Record approval date in `CHECKLIST-UI.md` once complete.
4. After approval, lock tokens in `libs/ui-kit/src/lib/tokens.css` and add unit tests verifying CSS exports.
