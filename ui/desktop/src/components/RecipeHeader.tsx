import type { RecipeHeaderProps } from './recipeHeaderProps';

import { defineMessages, useIntl } from '../i18n';
import type { NoMessageValues } from 'react-intl';

const i18n = defineMessages<{
  readonly "recipeLabel": NoMessageValues;
}>({
  recipeLabel: {
    id: 'recipeHeader.recipeLabel',
    defaultMessage: 'Recipe',
  },
});


export function RecipeHeader({ title }: RecipeHeaderProps) {
  const intl = useIntl();
  return (
    <div className="flex items-center justify-between px-4 py-2 border-b border-border-primary">
      <div className="flex items-center ml-6">
        <span className="w-2 h-2 rounded-full bg-green-500 mr-2" />
        <span className="text-sm">
          <span className="text-text-secondary">{intl.formatMessage(i18n.recipeLabel)}</span>{' '}
          <span className="text-text-primary">{title}</span>
        </span>
      </div>
    </div>
  );
}
