export async function safeJsonParse<T>(
  response: Response,
  errorMessage: string = 'Failed to parse server response'
): Promise<T> {
  try {
    return (await response.json()) as T;
  } catch (error) {
    if (error instanceof SyntaxError) {
      throw new Error(errorMessage, { cause: error });
    }
    throw error;
  }
}

export function errorMessage(err: unknown, default_value?: string): string {
  const acpData: unknown = acpErrorData(err);
  if (typeof acpData === 'string') {
    return acpData;
  }

  if (err instanceof Error) {
    return err.message;
  } else if (typeof err === 'object' && err !== null && 'message' in err) {
    return String(err.message);
  } else {
    return default_value ?? stringifyUnknown(err);
  }
}

function stringifyUnknown(value: unknown): string {
  if (typeof value === 'object' && value !== null) {
    try {
      return JSON.stringify(value);
    } catch {
      return Object.prototype.toString.call(value);
    }
  }

  return String(value);
}

function acpErrorData(err: unknown): unknown {
  if (typeof err !== 'object' || err === null) {
    return undefined;
  }

  const candidate: unknown = 'error' in err && isRecord(err.error) ? err.error : err;
  return isRecord(candidate) ? candidate.data : undefined;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

export function formatErrorForLogging(error: unknown): string {
  if (error instanceof Error) {
    return `${error.name}: ${error.message}${error.stack ? `\n${error.stack}` : ''}`;
  }
  if (typeof error === 'object' && error !== null) {
    try {
      return JSON.stringify(error, null, 2);
    } catch {
      return Object.prototype.toString.call(error);
    }
  }
  return String(error);
}

export async function compressImageDataUrl(dataUrl: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const img: HTMLImageElement = new globalThis.Image();
    img.onload = () => {
      const maxDim: number = 1024;
      const scale: number = Math.min(1, maxDim / Math.max(img.width, img.height));
      const width: number = Math.floor(img.width * scale);
      const height: number = Math.floor(img.height * scale);

      const canvas: HTMLCanvasElement = document.createElement('canvas');
      canvas.width = width;
      canvas.height = height;
      const ctx: CanvasRenderingContext2D | null = canvas.getContext('2d');
      if (!ctx) {
        reject(new Error('Failed to get canvas context'));
        return;
      }
      ctx.drawImage(img, 0, 0, width, height);

      resolve(canvas.toDataURL('image/jpeg', 0.85));
    };
    img.onerror = () => reject(new Error('Failed to load image'));
    img.src = dataUrl;
  });
}

export function formatAppName(name: string): string {
  return name
    .split(/[-_\s]+/)
    .filter((word: string): boolean => word.length > 0)
    .map((word: string): string => word.charAt(0).toUpperCase() + word.slice(1).toLowerCase())
    .join(' ');
}
