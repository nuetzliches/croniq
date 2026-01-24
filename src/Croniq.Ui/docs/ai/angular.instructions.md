# Angular Guidelines & Standards

## Signals Forms

We use the experimental `@angular/forms/signals` for type-safe, signal-based forms.

### State Management (Disabled/Readonly/Hidden)

Do **not** bind to `[disabled]` or `[attr.disabled]` in the template when using the `[field]` directive. The directive manages the DOM state exclusively to prevent conflicts.
Binding to `[disabled]` causes a compiler error (e.g. `ngtsc -998022`).

**Incorrect:**

```html
<!-- This will throw an error -->
<input [field]="form.myField" [disabled]="someCondition" />
```

**Correct:**
Handle the state in the form definition using the `disabled()`/`readonly()`/`hidden()` logic functions.

```typescript
import { computed } from '@angular/core';
import { disabled, form, hidden, readonly } from '@angular/forms/signals';

// ...

readonly disableCalendar = computed(() => this.calendarOptionsLoading() || this.calendarOptionsPermissionDenied());

readonly myForm = form(this.model, (f) => {
    // Apply static or reactive logic directly in the form definition.
    disabled(f.calendarId, () => this.disableCalendar());
    readonly(f.name, () => this.isEditMode());
    hidden(f.advancedOptions, () => !this.showAdvancedOptions());
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

## Route-bound Selection (Required)

List/detail pages must bind selection to query params to enable deep-linking and cross-page navigation.

**Rules:**

- Use `cq-data-grid` with `idKey`, `selectedId`, and `selectedIdChange`.
- Read the initial selection from `ActivatedRoute.queryParamMap`.
- On user selection, update the query param via `Router.navigate` using `queryParamsHandling: 'merge'` and `replaceUrl: true`.
- Keep selections in signals and derive detail data from the selected id.

**Example (Jobs):**

```ts
readonly selectedJobKey = signal<string | null>(null);

constructor() {
  this.route.queryParamMap
    .pipe(takeUntilDestroyed(this.destroyRef))
    .subscribe((params) => this.selectedJobKey.set(params.get('jobKey')));
}

setSelectedJobKey(value: string | number | null): void {
  const key = typeof value === 'string' ? value.trim() : value ? String(value) : null;
  this.selectedJobKey.set(key);
  void this.router.navigate([], {
    relativeTo: this.route,
    queryParams: { jobKey: key },
    queryParamsHandling: 'merge',
    replaceUrl: true,
  });
}
```

```html
<cq-data-grid
  [rows]="rows()"
  [rowKey]="rowKey"
  [idKey]="'jobKey'"
  [selectedId]="selectedJobKey()"
  (selectedIdChange)="setSelectedJobKey($event)"
></cq-data-grid>
```
