# Angular Implementation Guidelines

This document outlines the coding standards and patterns for Angular development in the Croniq UI.

## Forms (Signal Forms)

We use the **Signal Forms** API (`@angular/forms/signals`) for all new forms.
**Do not** use `ReactiveFormsModule` (`FormGroup`, `FormControl`) or `FormsModule` (`ngModel`) unless strictly necessary for legacy support.

### Pattern

1.  **Model**: Define the form state as a standard `signal`.
2.  **Form Definition**: Use the `form()` function to bind the signal to validation rules.
3.  **Template Binding**: Use the `[field]` directive to bind inputs to form fields.

### Example

```typescript
import { Component, signal } from '@angular/core';
import { Field, form, required } from '@angular/forms/signals';

@Component({
  selector: 'app-example',
  imports: [Field], // Import Field directive
  template: `
    <form (submit)="save()">
      <label>
        Username
        <input type="text" [field]="myForm.username" />
      </label>

      @if (myForm().invalid()) {
      <div class="error">Form is invalid</div>
      }
    </form>
  `,
})
export class ExampleComponent {
  // 1. Define model signal
  readonly model = signal({
    username: '',
    password: '',
  });

  // 2. Define form with validations
  readonly myForm = form(this.model, (f) => {
    required(f.username, { message: 'Username is required' });
    required(f.password);
  });

  save() {
    // 3. Check validity
    if (this.myForm().invalid()) {
      return;
    }

    const payload = this.model();
    // ...
  }
}
```

## Components

- **Standalone**: All components must be standalone (`imports: [...]`).
- **Change Detection**: Always use `ChangeDetectionStrategy.OnPush`.
- **Dependency Injection**: Use the `inject()` function for all dependencies. Avoid constructor injection.
- **Signals**:
  - Use `signal()` for mutable state.
  - Use `computed()` for derived state.
  - Expose signals as `readonly` properties.
