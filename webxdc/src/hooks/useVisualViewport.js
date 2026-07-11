import { useEffect } from "preact/hooks";

/**
 * Keep the app inside the *visible* viewport when the mobile keyboard opens.
 * Sets CSS vars used by #app:
 *   --app-height, --app-top, --app-left, --app-width
 * and toggles html.keyboard-open when the keyboard is covering space.
 */
export function useVisualViewport() {
  useEffect(() => {
    const root = document.documentElement;
    let raf = 0;

    const apply = () => {
      raf = 0;
      const vv = window.visualViewport;
      if (!vv) {
        root.style.setProperty("--app-height", `${window.innerHeight}px`);
        root.style.setProperty("--app-width", `${window.innerWidth}px`);
        root.style.setProperty("--app-top", "0px");
        root.style.setProperty("--app-left", "0px");
        root.classList.remove("keyboard-open");
        return;
      }

      // Visible area (keyboard already excluded from height on most mobile browsers).
      const height = Math.max(1, Math.round(vv.height));
      const width = Math.max(1, Math.round(vv.width));
      const top = Math.max(0, Math.round(vv.offsetTop));
      const left = Math.max(0, Math.round(vv.offsetLeft));

      root.style.setProperty("--app-height", `${height}px`);
      root.style.setProperty("--app-width", `${width}px`);
      root.style.setProperty("--app-top", `${top}px`);
      root.style.setProperty("--app-left", `${left}px`);

      // Heuristic: keyboard open when visible height is clearly less than layout height.
      const covered = window.innerHeight - vv.height - vv.offsetTop;
      const open = covered > 80 || top > 0;
      root.classList.toggle("keyboard-open", open);

      // Keep page scrolled to the visual viewport origin (iOS sometimes shifts body).
      if (window.scrollY !== 0 || window.scrollX !== 0) {
        window.scrollTo(0, 0);
      }
    };

    const schedule = () => {
      if (raf) cancelAnimationFrame(raf);
      raf = requestAnimationFrame(apply);
    };

    apply();
    window.addEventListener("resize", schedule);
    window.addEventListener("orientationchange", schedule);
    window.visualViewport?.addEventListener("resize", schedule);
    window.visualViewport?.addEventListener("scroll", schedule);
    // Focus in terminal often opens the keyboard a frame later.
    window.addEventListener("focusin", schedule);
    window.addEventListener("focusout", schedule);

    return () => {
      if (raf) cancelAnimationFrame(raf);
      window.removeEventListener("resize", schedule);
      window.removeEventListener("orientationchange", schedule);
      window.visualViewport?.removeEventListener("resize", schedule);
      window.visualViewport?.removeEventListener("scroll", schedule);
      window.removeEventListener("focusin", schedule);
      window.removeEventListener("focusout", schedule);
      root.classList.remove("keyboard-open");
    };
  }, []);
}
