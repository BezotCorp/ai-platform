import type { ImageMessageContent, Message } from '.';

export interface ImageData {
  data: string;
  mimeType: string;
  _meta?: ImageMessageContent['_meta'];
  annotations?: ImageMessageContent['annotations'];
}

export function imageDataFromMessage(message: Message): ImageData[] {
  return message.content
    .filter((c): c is ImageMessageContent => c.type === 'image')
    .map((c) => ({
      data: c.data,
      mimeType: c.mimeType,
      ...(c._meta ? { _meta: c._meta } : {}),
      ...(c.annotations ? { annotations: c.annotations } : {}),
    }));
}
