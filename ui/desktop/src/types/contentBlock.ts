import type { RawAudioContent } from './rawAudioContent';
import type { RawEmbeddedResource } from './rawEmbeddedResource';
import type { RawImageContent } from './rawImageContent';
import type { RawResource } from './rawResource';
import type { RawTextContent } from './rawTextContent';

export type ContentBlock =
  | ({ type: 'text' } & RawTextContent)
  | ({ type: 'image' } & RawImageContent)
  | ({ type: 'resource' } & RawEmbeddedResource)
  | ({ type: 'audio' } & RawAudioContent)
  | ({ type: 'resource_link' } & RawResource);
