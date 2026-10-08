export const configLabels: Record<string, string> = {
  // goose settings
  BCAIP_PROVIDER: 'Provider',
  BCAIP_MODEL: 'Model',
  BCAIP_TEMPERATURE: 'Temperature',
  BCAIP_MODE: 'Mode',
  BCAIP_TOOLSHIM: 'Tool Shim',
  BCAIP_TOOLSHIM_OLLAMA_MODEL: 'Tool Shim Ollama Model',
  BCAIP_CLI_MIN_PRIORITY: 'CLI Min Priority',
  BCAIP_ALLOWLIST: 'Allow List',
  BCAIP_RECIPE_GITHUB_REPO: 'Recipe GitHub Repo',

  // security settings
  SECURITY_PROMPT_ENABLED: 'Prompt Injection Detection Enabled',
  SECURITY_PROMPT_THRESHOLD: 'Prompt Injection Detection Threshold',
  SECURITY_PROMPT_CLASSIFIER_ENABLED: 'ML-based Prompt Injection Detection Enabled',
  SECURITY_PROMPT_CLASSIFIER_MODEL: 'ML-based Prompt Injection Detection Model',
  SECURITY_PROMPT_CLASSIFIER_ENDPOINT: 'ML Classification Endpoint',
  SECURITY_PROMPT_CLASSIFIER_TOKEN: 'ML Classification API Token',

  // openai
  OPENAI_API_KEY: 'OpenAI API Key',
  OPENAI_HOST: 'OpenAI Host',
  OPENAI_BASE_PATH: 'OpenAI Base Path',

  // groq
  GROQ_API_KEY: 'Groq API Key',

  // openrouter
  OPENROUTER_API_KEY: 'OpenRouter API Key',

  // anthropic
  ANTHROPIC_API_KEY: 'Anthropic API Key',
  ANTHROPIC_HOST: 'Anthropic Host',

  // google
  GOOGLE_API_KEY: 'Google API Key',

  // databricks
  DATABRICKS_HOST: 'Databricks Host',

  // ollama
  OLLAMA_HOST: 'Ollama Host',

  // ollama cloud
  OLLAMA_CLOUD_API_KEY: 'Ollama Cloud API Key',

  // azure openai
  AZURE_OPENAI_API_KEY: 'Azure OpenAI API Key',
  AZURE_OPENAI_ENDPOINT: 'Azure OpenAI Endpoint',
  AZURE_OPENAI_DEPLOYMENT_NAME: 'Azure OpenAI Deployment Name',
  AZURE_OPENAI_API_VERSION: 'Azure OpenAI API Version',

  // gcp vertex
  GCP_PROJECT_ID: 'GCP Project ID',
  GCP_LOCATION: 'GCP Location',

  // snowflake
  SNOWFLAKE_HOST: 'Snowflake Host',
  SNOWFLAKE_TOKEN: 'Snowflake Token',

  // github copilot
  GITHUB_COPILOT_HOST: 'Custom GitHub Host',
  GITHUB_COPILOT_CLIENT_ID: 'Custom GitHub OAuth Client ID',
  GITHUB_COPILOT_TOKEN_URL: 'Custom GitHub Copilot Token URL',
};

// UI placeholder text shown in empty configuration fields.
// These values are examples only and are never used as actual configuration values.
export const configFieldPlaceholders = {
  GITHUB_COPILOT_HOST: 'my-enterprise.ghe.com',
  GITHUB_COPILOT_CLIENT_ID: 'Iv1.xxxxxxxxxxxxxxxx',
  GITHUB_COPILOT_TOKEN_URL: 'https://my-enterprise.ghe.com/api/copilot_internal/v2/token',
} as const satisfies Readonly<Record<string, string>>;

export const getConfigFieldPlaceholder = (key: string): string | undefined => {
  return configFieldPlaceholders[key as keyof typeof configFieldPlaceholders];
};

export const providerPrefixes: Record<string, string[]> = {
  openai: ['OPENAI_'],
  anthropic: ['ANTHROPIC_'],
  google: ['GOOGLE_'],
  groq: ['GROQ_'],
  databricks: ['DATABRICKS_'],
  databricks_v2: ['DATABRICKS_'],
  openrouter: ['OPENROUTER_'],
  ollama: ['OLLAMA_'],
  azure_openai: ['AZURE_'],
  gcp_vertex_ai: ['GCP_'],
  snowflake: ['SNOWFLAKE_'],
  github_copilot: ['GITHUB_COPILOT_'],
};

export const getUiNames = (key: string): string => {
  if (configLabels[key]) {
    return configLabels[key];
  }
  return key
    .split('_')
    .map((word) => word.charAt(0) + word.slice(1).toLowerCase())
    .join(' ');
};
