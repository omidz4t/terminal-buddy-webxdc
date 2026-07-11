import { useEffect, useState } from "preact/hooks";

/**
 * Wait until Delta Chat injects window.webxdc with joinRealtimeChannel.
 * @param {number} [timeoutMs]
 * @returns {{ webxdc: any | null, error: Error | null, ready: boolean }}
 */
export function useWebxdc(timeoutMs = 8000) {
  const [webxdc, setWebxdc] = useState(null);
  const [error, setError] = useState(null);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const t0 = Date.now();

    const tick = () => {
      if (cancelled) return;
      const w = window.webxdc;
      if (w && typeof w.joinRealtimeChannel === "function") {
        setWebxdc(w);
        setReady(true);
        return;
      }
      if (Date.now() - t0 > timeoutMs) {
        const keys = w ? Object.keys(w).join(", ") : "(no window.webxdc)";
        setError(new Error("joinRealtimeChannel missing. webxdc keys: " + keys));
        setReady(true);
        return;
      }
      setTimeout(tick, 30);
    };
    tick();
    return () => {
      cancelled = true;
    };
  }, [timeoutMs]);

  return { webxdc, error, ready };
}
