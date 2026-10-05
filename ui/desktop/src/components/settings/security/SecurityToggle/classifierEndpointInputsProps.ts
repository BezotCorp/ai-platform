export interface ClassifierEndpointInputsProps {
  endpointValue: string;
  tokenValue: string;
  onEndpointChange: (value: string) => void;
  onTokenChange: (value: string) => void;
  onEndpointBlur: (value: string) => void;
  onTokenBlur: (value: string) => void;
  disabled: boolean;
  endpointPlaceholder: string;
  tokenPlaceholder: string;
  endpointLabel?: string;
  endpointDescription?: string;
  tokenLabel?: string;
  tokenDescription?: string;
}
