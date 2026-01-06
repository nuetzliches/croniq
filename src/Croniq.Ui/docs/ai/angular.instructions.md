# Angular Guidelines & Standards

## Signals Forms

We use the experimental `@angular/forms/signals` for type-safe, signal-based forms.

### State Management (Disabled/Readonly)

Do **not** bind to `[disabled]` or `[attr.disabled]` in the template when using the `[field]` directive. The directive manages the DOM state exclusively to prevent conflicts.
Binding to `[disabled]` causes a compiler error (e.g. `ngtsc -998022`).

**Incorrect:**

```html
<!-- This will throw an error -->
<input [field]="form.myField" [disabled]="someCondition" />
```

**Correct:**
Handle the state in the form definition using the `disabled()` modifier function.

```typescript
import { disabled, form } from '@angular/forms/signals';

// ...

readonly myForm = form(this.model, (f) => {
    // Apply static initial state
    if (this.someCondition) {
        disabled(f.myField);
    }

    // For reactive state changes, use effects or update the control state programmatically
    // (Pattern to be confirmed depending on exact library version)
});
```

### Validation

Use validators like `required()` in the form setup block, rather than template attributes.

```typescript
readonly myForm = form(this.model, (f) => {
    required(f.myField, { message: 'Field is required' });
});
```
