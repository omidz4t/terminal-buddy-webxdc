import { useRef, useState } from "preact/hooks";

export function ClipboardRequest({ text, onClose }) {
  const field = useRef(null);
  const [message, setMessage] = useState("");
  const copy = async () => {
    // Called directly from a local click, preserving browser user activation.
    try {
      if (!navigator.clipboard?.writeText) throw new Error("Clipboard unavailable");
      await navigator.clipboard.writeText(text);
      setMessage("Copied to your clipboard");
    } catch {
      field.current?.focus();
      field.current?.select();
      try {
        if (document.execCommand("copy")) {
          setMessage("Copied to your clipboard");
          return;
        }
      } catch { /* Keep the selectable text available. */ }
      setMessage("Select the text below and use your device’s Copy action.");
    }
  };
  return (
    <section class="clipboard-request" aria-label="Terminal copy request">
      <strong>Terminal requested a copy</strong>
      <textarea ref={field} value={text} readOnly aria-label="Text to copy"
        onFocus={(event) => event.currentTarget.select()} />
      <div class="clipboard-actions">
        <button type="button" onClick={copy}>Copy to clipboard</button>
        <button type="button" onClick={onClose}>Dismiss</button>
      </div>
      <div role="status" aria-live="polite">{message}</div>
    </section>
  );
}
