import { useEffect } from "react";
const focusable = 'button:not(:disabled), a[href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), summary, [tabindex="0"]';
function controls(root: HTMLElement) {
  return [...root.querySelectorAll<HTMLElement>(focusable)].filter((element) => element.getClientRects().length > 0);
}
export function useOverlayAccessibility() {
  useEffect(() => {
    let current: HTMLElement | null = null;
    let previousFocus: HTMLElement | null = null;
    const update = () => {
      const overlays = [...document.querySelectorAll<HTMLElement>('[role="dialog"], .fixed.inset-0 > section, [role="menu"]')];
      const next = overlays.filter((element) => element.getClientRects().length > 0).at(-1) ?? null;
      if (next === current) return;
      if (!next) { if (previousFocus?.isConnected) previousFocus.focus({ preventScroll: true }); current = null; return; }
      if (!current) previousFocus = document.activeElement as HTMLElement;
      current = next;
      if (next.getAttribute("role") !== "menu") {
        next.setAttribute("role", "dialog");
        next.setAttribute("aria-modal", "true");
        if (!next.hasAttribute("aria-label") && !next.hasAttribute("aria-labelledby")) {
          const heading = next.querySelector<HTMLElement>('h1,h2,h3,header,.font-semibold');
          next.setAttribute("aria-label", heading?.textContent?.trim() || "Operation details");
        }
      }
      controls(next)[0]?.focus({ preventScroll: true });
    };
    function dismiss() {
      if (!current) return;
      if (current.getAttribute("role") === "menu") { document.body.click(); document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); }
      else current.querySelector<HTMLElement>('button[title="Close"], button[aria-label^="Close"]')?.click();
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (!current) return;
      const items = controls(current);
      const menu = current.getAttribute("role") === "menu";
      if (event.key === "Escape") { event.preventDefault(); dismiss(); }
      if (menu && ["ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) {
        event.preventDefault(); const index = items.indexOf(document.activeElement as HTMLElement);
        const target = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : (index + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
        items[target]?.focus();
      }
      if (event.key === "Tab") {
        if (menu) { dismiss(); return; }
        const first = items[0], last = items.at(-1);
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }
    };
    update();
    const observer = new MutationObserver(update);
    observer.observe(document.body, { childList: true, subtree: true });
    document.addEventListener("keydown", onKeyDown);
    return () => { observer.disconnect(); document.removeEventListener("keydown", onKeyDown); };
  }, []);
}
