import React from 'react';
import type { ProviderDetails } from '../../../../../../../types/providerDetails';
import type { ValidationErrors } from '../DefaultProviderSetupForm/validationErrors';
import type { ConfigInput } from '../DefaultProviderSetupForm/configInput';

export interface DefaultProviderSetupFormProps {
  configValues: Record<string, ConfigInput>;
  setConfigValues: React.Dispatch<React.SetStateAction<Record<string, ConfigInput>>>;
  provider: ProviderDetails;
  validationErrors: ValidationErrors;
  showOptions?: boolean;
}
