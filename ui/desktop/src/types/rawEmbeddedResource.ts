import type { JsonObject } from './jsonObject';
import type { ResourceContents } from './resourceContents';

export type RawEmbeddedResource = {
  _meta?: JsonObject;
  resource: ResourceContents;
};
