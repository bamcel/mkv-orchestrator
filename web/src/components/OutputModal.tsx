import { Copy, X } from "lucide-react";

type OutputModalProps = {
  title: string;
  content: string;
  onClose: () => void;
};

export function OutputModal({ title, content, onClose }: OutputModalProps) {
  async function copyContent() {
    await navigator.clipboard.writeText(content);
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-6">
      <section className="flex h-[min(760px,calc(100vh-48px))] w-[min(1040px,calc(100vw-48px))] flex-col overflow-hidden rounded-lg border-2 border-window bg-card shadow-[0_30px_90px_rgba(0,0,0,0.55)]">
        <div className="flex h-10 shrink-0 items-center justify-between border-b border-border bg-window px-4">
          <div className="text-sm font-semibold text-muted">{title}</div>
          <button
            type="button"
            onClick={onClose}
            className="inline-flex h-7 w-7 items-center justify-center rounded-md text-muted transition hover:bg-button-hover hover:text-text"
            title="Close"
          >
            <X size={16} />
          </button>
        </div>

        <div className="flex min-h-0 flex-1 flex-col p-5">
          <div className="mb-3 flex justify-end">
            <button
              type="button"
              onClick={copyContent}
              className="inline-flex h-9 items-center gap-2 rounded-md border border-border bg-button px-3 text-sm font-semibold text-muted transition hover:bg-button-hover hover:text-text"
            >
              <Copy size={15} />
              Copy
            </button>
          </div>
          <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-words rounded-md border border-border bg-input p-4 font-mono text-xs leading-6 text-muted">
            {content}
          </pre>
        </div>
      </section>
    </div>
  );
}
