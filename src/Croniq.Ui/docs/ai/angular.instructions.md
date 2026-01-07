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

### Form Submission

Use the `submit` helper to handle validation checks automatically. Always accept `SubmitEvent`, call `preventDefault()`, and use `await` with the `submit` function.

**Correct Pattern:**

```typescript
import { submit } from '@angular/forms/signals';

// ...

async onSubmit(event: SubmitEvent) {
    event.preventDefault(); // Stop creating query params in URL

    await submit(this.myForm, async () => {
        const value = this.model();
        await this.save(value);
    });
}
```

**Template:**

```html
<form (submit)="onSubmit($event)" novalidate>
  <!-- ... -->
  <button type="submit">Save</button>
</form>
```

### Dialogs

We use `@angular/cdk/dialog` for modal dialogs. Components opened in dialogs should use dependency injection to retrieve references and data.

**Standard Pattern:**

1. **Injection:** Use `inject(DialogRef)` to control the dialog.
2. **Data:** Use `inject(DIALOG_DATA)` to receive inputs.
3. **Closing:** Call `dialogRef.close(result)` to return data.

```typescript
import { Component, inject } from '@angular/core';
import { DialogRef, DIALOG_DATA } from '@angular/cdk/dialog';

@Component({
  selector: 'app-example-dialog',
  standalone: true,
  templateUrl: './example-dialog.component.html',
})
export class ExampleDialogComponent {
  private readonly dialogRef = inject(DialogRef);
  readonly data = inject<MyDataType>(DIALOG_DATA);

  close() {
    this.dialogRef.close();
  }

  save(result: any) {
    this.dialogRef.close(result);
  }
}
```
