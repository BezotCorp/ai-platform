import type {
  RawAudioContent,
  RawEmbeddedResource,
  RawImageContent,
  RawResource,
  RawTextContent,
} from '.';

export type ContentBlock =
  | ({ type: 'text' } & RawTextContent)
  | ({ type: 'image' } & RawImageContent)
  | ({ type: 'resource' } & RawEmbeddedResource)
  | ({ type: 'audio' } & RawAudioContent)
  | ({ type: 'resource_link' } & RawResource);
