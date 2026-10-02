export default function useBaseUrl(path = "/"): string {
  const base = import.meta.env.BASE_URL || "/";

  if (/^[a-z]+:/i.test(path)) {
    return path;
  }

  const normalizedBase = base.endsWith("/") ? base : `${base}/`;
  const normalizedPath = path.startsWith("/") ? path.slice(1) : path;

  return `${normalizedBase}${normalizedPath}`;
}
