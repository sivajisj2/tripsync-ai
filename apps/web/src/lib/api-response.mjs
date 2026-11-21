export async function parseApiResponse(response) {
  if (!response.ok) {
    const body = await response.text();
    const isHtml = response.headers.get("content-type")?.includes("text/html") || /^\s*<!doctype html|^\s*<html/i.test(body);
    throw new Error(isHtml ? `The server is temporarily unavailable (HTTP ${response.status}). Please retry in a moment.` : body || `Request failed (HTTP ${response.status})`);
  }

  if (response.status === 204 || response.status === 205) return undefined;

  const body = await response.text();
  if (!body.trim()) return undefined;

  try {
    return JSON.parse(body);
  } catch {
    throw new Error(`Invalid JSON response (HTTP ${response.status})`);
  }
}

export async function apiRequest(path, options = {}) {
  const { headers, ...requestOptions } = options;
  const mergedHeaders = new Headers(headers);
  if (!mergedHeaders.has("Content-Type")) mergedHeaders.set("Content-Type", "application/json");
  const response = await fetch(`/api${path}`, {
    ...requestOptions,
    headers: mergedHeaders,
  });
  return parseApiResponse(response);
}
