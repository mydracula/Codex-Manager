export function normalizeUpstreamProxyUrl(value) {
  if (value == null) {
    return "";
  }

  let normalized = String(value).trim();
  if (!normalized) {
    return "";
  }

  if (normalized.startsWith("http://socks")) {
    normalized = normalized.slice("http://".length);
  } else if (normalized.startsWith("https://socks")) {
    normalized = normalized.slice("https://".length);
  }

  if (normalized.startsWith("socks5://")) {
    return normalized.replace("socks5://", "socks5h://");
  }
  if (normalized.startsWith("socks://")) {
    return normalized.replace("socks://", "socks5h://");
  }

  return normalized;
}
