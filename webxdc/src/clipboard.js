// OSC 52: selection ; base64 text. Queries must never read the local clipboard.
export function decodeClipboardRequest(data) {
  if (typeof data !== "string" || data.length > 140000) return null;
  const separator = data.indexOf(";");
  if (separator < 0 || !/^[cpsq0-7]*$/.test(data.slice(0, separator))) return null;
  const encoded = data.slice(separator + 1);
  if (!encoded || encoded === "?" || !/^[A-Za-z0-9+/]*={0,2}$/.test(encoded)) return null;
  try {
    const bytes = Uint8Array.from(atob(encoded), (char) => char.charCodeAt(0));
    if (bytes.length > 100000) return null;
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

export function installClipboardHandler(term, onCopyRequest) {
  return term.parser.registerOscHandler(52, (data) => {
    const text = decodeClipboardRequest(data);
    if (text !== null) onCopyRequest(text);
    return true;
  });
}
