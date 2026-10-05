import type { ElementType } from 'react';
import type { Parameter } from '../../../../recipe';
import type { RecipeFormData } from './recipeFormData';

export interface RecipeFormApi {
  state: {
    values: RecipeFormData;
  };
  store: {
    subscribe(
      listener: () => void
    ): {
      unsubscribe: () => void;
    };
  };
  Field: ElementType;
  setFieldValue(
    name: 'parameters',
    value: Parameter[]
  ): void;
}
