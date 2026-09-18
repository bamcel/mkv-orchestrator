import { useEffect, useState, type ReactNode } from "react";
export function MobileOptions({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(true);
  useEffect(() => {
    if (!window.matchMedia) return;
    const media = window.matchMedia("(min-width: 768px) and (pointer: fine), (min-width: 1024px)");
    const update = () => { if (media.matches) setOpen(true); };
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  return <details className="mobile-options min-h-0 flex-1 overflow-y-auto" open={open} onToggle={(event) => setOpen(event.currentTarget.open)}>
    <summary className="mobile-options-summary cursor-pointer font-semibold">Operation options</summary>
    <div className="mobile-options-content">{children}</div>
  </details>;
}
