import type { ImageData } from '../../types/imageData';

export interface QueuedMessage {
  id: string;
  content: string;
  timestamp: number;
  images: ImageData[];
}
