export type ConfigKey = {
  default?: string | null;
  device_code_flow?: boolean;
  name: string;
  oauth_flow: boolean;
  primary?: boolean;
  required: boolean;
  secret: boolean;
};
