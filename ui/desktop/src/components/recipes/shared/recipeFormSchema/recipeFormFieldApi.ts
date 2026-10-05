export interface RecipeFormFieldApi<TValue> {
  state: {
    value: TValue;
    meta: {
      errors: unknown[];
    };
  };
  handleBlur(): void;
  handleChange(value: TValue): void;
}
