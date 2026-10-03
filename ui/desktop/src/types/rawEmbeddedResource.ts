import type { JsonObject, ResourceContents } from '.';

export type RawEmbeddedResource = {
  _meta?: JsonObject;
  resource: ResourceContents;
};
