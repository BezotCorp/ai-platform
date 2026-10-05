import type { DownloadModelRequest } from '../../../../acp/downloadModelRequest';


export interface Props {
  onDownloadStarted: (modelId: string, request: DownloadModelRequest) => void;
  /** Model IDs (repo:quant) with an active download in progress */
  activeDownloadIds?: Set<string>;
  /** Model IDs (repo:quant) confirmed downloaded on disk */
  downloadedModelIds?: Set<string>;
}
