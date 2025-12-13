# Croniq UI – ARIA Playbook

_Source: [Angular ARIA guidance](https://angular.dev/guide/aria/overview). Follow this playbook whenever you touch markup, styles, or interaction logic. Reference it in every pull request checklist._

## 1. Required Landmarks & Roles

- Expose at least one top-level landmark (`<header>`, `<nav>`, `<main>`, `<footer>`). Use `aria-label` only when the semantic element needs disambiguation.
- Dialogs (e.g., the command palette) **must** use `role="dialog"` or `role="alertdialog"`, `aria-modal="true"`, and `aria-labelledby` pointing at a visible title.
- Lists with keyboard selection (command results, entity pickers) act as `role="listbox"` with child `role="option"` items and an `aria-activedescendant` on the focusable element.
- Loading overlays announce themselves via `aria-live="polite"` and must include progress text ("Fetching jobs…").

## 2. Focus & Keyboard Order

- Preserve logical tab order: trigger → dialog → close button → list/search → return focus to trigger on close.
- Keyboard combos:
  - `⌘/Ctrl + K` opens the palette.
  - `Esc` closes any transient UI (palette, dropdown, toast) and returns focus.
  - `↑/↓` cycle options with wrap-around; `Enter` activates the current option; `Shift+Enter` can reserve advanced actions later.
- Use `@HostListener('keydown')` **only** if host metadata cannot express the binding; prefer dedicated controller methods that receive the event from the template.

## 3. Live Regions & Status Text

- Attach a visually hidden (`.sr-only`) element with `aria-live="polite"` for announcing list counts or filter states ("5 commands available").
- Use `aria-live="assertive"` sparingly (fatal errors only). Pair assertive regions with helpful remediation text.
- Keep announcements concise (<120 chars) and avoid repeating the same text if nothing changed.

## 4. Testing Workflow

1. Manual keyboard pass (Tab, Shift+Tab, Enter, space, arrow keys, Esc). Confirm focus is never lost.
2. Run AXE (browser extension or CI) and resolve violations before merging.
3. Smoke-test with NVDA or VoiceOver for new interactive components. Confirm role/name/value triads read as expected.
4. For visual diffs driven by Tailwind utilities, verify that semantics still match the headless controller output.

## 5. Command Palette Expectations

- State lives inside `CommandPaletteController` (signals for `isOpen`, `query`, `activeIndex`, `filteredCommands`).
- Presenters (Angular components, or future Tailwind-styled wrappers) **must**:
  - Render a `role="dialog"` container with trapped focus and ESC handling.
  - Provide a searchable input tied to `aria-activedescendant` updates coming from the controller.
  - Emit live-region updates when results change.
  - Reuse controller helpers instead of duplicating filtering or navigation logic.

## 6. Tailwind Styling Notes

- Follow [Angular’s Tailwind integration guide](https://angular.dev/guide/tailwind). Define utilities in `tailwind.config.[cm]js` that map to Croniq tokens (`--cq-*`).
- Headless components expose state via classes/attributes (e.g., `[aria-selected="true"]`). Tailwind layers should style those selectors, keeping ARIA hooks intact.
- When adding Tailwind utilities, keep semantics untouched—no `div`-only replacements for semantic elements.

## 7. Pull Request Hook

Every PR that touches UI **must** answer the following prompts in the template:

- ✅ Which ARIA roles/focus paths were added or changed?
- ✅ How was keyboard + screen reader support validated?
- ✅ Did you reuse `CommandPaletteController` (or document why not)?
- ✅ Did Tailwind utilities follow the Angular guide and Croniq token plan?

Document answers in the PR body and link to related test evidence or recordings when possible.
