## Summary

- Describe the scope of this change (feature, fix, refactor) and call out any cross-team impacts.

## ARIA & Accessibility

- [ ] Reviewed the [ARIA playbook](../docs/ai/aria.md) and documented the roles/focus order updates in this PR.
- [ ] Verified keyboard & screen reader behavior (Tab/Shift+Tab, arrow keys, Esc, announcements).

## Command Palette / Headless Components

- [ ] Reused `CommandPaletteController` (or explained why another controller/store was needed).
- [ ] Announced result counts via live regions and kept `aria-activedescendant` in sync.

## Tailwind & Styling

- [ ] Followed [Angular Tailwind guidance](https://angular.dev/guide/tailwind) for any new utility classes or tokens.
- [ ] Ensured semantic elements + ARIA hooks remain intact after styling changes.

## Testing

- [ ] `npm run test`
- [ ] Additional checks (lint, e2e, visual) if applicable:

```bash
# Commands executed locally
# e.g. npm run lint
```
