import { useEffect, useId, useRef, type ReactNode } from "react";
import { ModalBody, ModalFrame } from "../../components/ui/ModalFrame";
import { Button } from "../../components/ui/Button";

export function FeedbackDialog({ title, onClose, busy = false, children }: {
  title: string;
  onClose: () => void;
  busy?: boolean;
  children: ReactNode;
}) {
  const titleId = useId();
  const content = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  close.current = onClose;
  const isBusy = useRef(busy);
  isBusy.current = busy;
  useEffect(() => {
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const panel = content.current;
    panel?.querySelector<HTMLElement>("button, input, select, textarea, a[href]")?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        if (!isBusy.current) close.current();
      }
      if (event.key !== "Tab" || !panel) return;
      const controls = [...panel.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], [tabindex='0']")]
        .filter((element) => element.getClientRects().length > 0);
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (!first || !last) { event.preventDefault(); panel.focus(); return; }
      if (event.shiftKey && (document.activeElement === first || !panel.contains(document.activeElement))) {
        event.preventDefault(); last.focus();
      } else if (!event.shiftKey && (document.activeElement === last || !panel.contains(document.activeElement))) {
        event.preventDefault(); first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("keydown", onKeyDown, true);
      if (previousFocus?.isConnected) previousFocus.focus();
    };
  }, []);
  return (
    <ModalFrame labelledBy={titleId} panelClassName="glass-panel pixel-frame max-w-2xl" zIndexClassName="z-[100]">
      <div ref={content} tabIndex={-1} className="flex min-h-0 flex-1 flex-col outline-none feedback-ui [&_button]:min-h-11">
        <header className="flex shrink-0 items-center justify-between gap-3 border-b border-[#3e4270] p-4">
          <h2 id={titleId} className="min-w-0 break-words font-display text-sm leading-relaxed text-white">{title}</h2>
          <Button type="button" variant="ghost" className="min-h-11 shrink-0" disabled={busy} onClick={onClose} aria-label={`Close ${title}`}>Close</Button>
        </header>
        <ModalBody className="space-y-5 p-4 text-base leading-relaxed text-[#c6dbf4] sm:p-5">{children}</ModalBody>
      </div>
    </ModalFrame>
  );
}
