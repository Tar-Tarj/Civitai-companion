import { backend } from "./bridge";
import { isAllowedImageUrl } from "./dom";

const cache = new Map<string, Promise<string | null>>();
const MAX_CACHED_IMAGES = 320;
let generation = 0;

export function clearImageCache(): void {
  generation += 1;
  for (const pending of cache.values()) {
    void pending.then((url) => { if (url) URL.revokeObjectURL(url); });
  }
  cache.clear();
}

function decodeBase64(value: string): Uint8Array {
  const binary = window.atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

async function fetchObjectUrl(url: string): Promise<string | null> {
  if (!isAllowedImageUrl(url)) return null;
  const requestedGeneration = generation;
  try {
    const payload = await backend.fetchImage(url);
    if (requestedGeneration !== generation) return null;
    const bytes = decodeBase64(payload.data);
    const buffer = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
    return URL.createObjectURL(new Blob([buffer], { type: payload.mimeType }));
  } catch {
    return null;
  }
}

function cachedObjectUrl(url: string): Promise<string | null> {
  const existing = cache.get(url);
  if (existing) {
    cache.delete(url);
    cache.set(url, existing);
    return existing;
  }
  if (cache.size >= MAX_CACHED_IMAGES) {
    const oldest = cache.keys().next().value as string | undefined;
    if (oldest) {
      const evicted = cache.get(oldest);
      cache.delete(oldest);
      void evicted?.then((objectUrl) => { if (objectUrl) URL.revokeObjectURL(objectUrl); });
    }
  }
  const pending = fetchObjectUrl(url);
  cache.set(url, pending);
  return pending;
}

export function loadCivitaiImage(
  image: HTMLImageElement,
  url: string,
  onFailure: () => void,
): void {
  image.alt = "";
  image.referrerPolicy = "no-referrer";
  const requestedGeneration = generation;
  void cachedObjectUrl(url).then((objectUrl) => {
    if (requestedGeneration !== generation) return;
    if (!image.isConnected || !objectUrl) {
      if (image.isConnected) onFailure();
      return;
    }
    image.addEventListener("error", onFailure, { once: true });
    image.src = objectUrl;
  });
}
