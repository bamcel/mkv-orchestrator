import type { RefObject } from "react";
export function PanelDivider({ containerRef, value, onChange }: { containerRef: RefObject<HTMLDivElement | null>; value: number; onChange: (value: number) => void }) {
  function update(next: number) { onChange(Math.max(25, Math.min(75, next))); }
  return <div role="separator" aria-label="Resize file and details panels" aria-orientation="horizontal" aria-valuemin={25} aria-valuemax={75} aria-valuenow={Math.round(value)} tabIndex={0}
    onKeyDown={(event) => { if (["ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) { event.preventDefault(); update(event.key === "Home" ? 25 : event.key === "End" ? 75 : value + (event.key === "ArrowUp" ? -5 : 5)); } }}
    onPointerDown={(event) => { event.currentTarget.setPointerCapture(event.pointerId); }}
    onPointerMove={(event) => { if (!event.currentTarget.hasPointerCapture(event.pointerId)) return; const bounds = containerRef.current?.getBoundingClientRect(); if (bounds?.height) update((event.clientY - bounds.top) / bounds.height * 100); }}
    onPointerUp={(event) => { if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId); }}
    className="flex cursor-row-resize touch-none items-center justify-center rounded focus-visible:outline-accent hover:bg-selected"><span className="h-1 w-12 rounded bg-border-strong" /></div>;
}
