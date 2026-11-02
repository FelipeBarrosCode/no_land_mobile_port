import { useEffect, useId, useRef, type ReactNode } from "react";

interface Props {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
}

export function MobileMenuButton({
  label,
  onClick,
  expanded,
}: {
  label: string;
  onClick: () => void;
  expanded: boolean;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-expanded={expanded}
      onClick={onClick}
      className="mobile-menu-button inline-flex size-11 shrink-0 items-center justify-center border border-[#4a5180] bg-[#11162f] text-xl text-neon-cyan shadow-[inset_0_0_0_1px_#080b19] md:hidden"
    >
      <span aria-hidden="true">☰</span>
    </button>
  );
}

export function MobileSideMenu({ open, title, onClose, children }: Props) {
  const panel = useRef<HTMLElement>(null);
  const close = useRef(onClose);
  close.current = onClose;
  const titleId = useId();
  useEffect(() => {
    if (!open) return;
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const menu = panel.current;
    menu?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        close.current();
      }
      if (event.key !== "Tab" || !menu) return;
      const controls = [...menu.querySelectorAll<HTMLElement>("button:not(:disabled), a[href], input:not(:disabled), select:not(:disabled), [tabindex='0']")]
        .filter((element) => element.getClientRects().length > 0);
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (!first || !last) { event.preventDefault(); menu.focus(); return; }
      if (event.shiftKey && (document.activeElement === first || !menu.contains(document.activeElement))) {
        event.preventDefault(); last.focus();
      } else if (!event.shiftKey && (document.activeElement === last || !menu.contains(document.activeElement))) {
        event.preventDefault(); first.focus();
      }
    };
    document.addEventListener("keydown", handleKey, true);
    const desktopLayout = window.matchMedia("(min-width: 768px)");
    const closeOnWideLayout = () => { if (desktopLayout.matches) close.current(); };
    desktopLayout.addEventListener("change", closeOnWideLayout);
    closeOnWideLayout();
    return () => {
      document.removeEventListener("keydown", handleKey, true);
      desktopLayout.removeEventListener("change", closeOnWideLayout);
      if (previousFocus?.isConnected) previousFocus.focus();
    };
  }, [open]);

  // A closed drawer must not leave the feedback entry (or other menu actions) tabbable off-screen.
  if (!open) return null;
  return (
    <div
      className={`fixed inset-0 z-[90] md:hidden ${open ? "pointer-events-auto" : "pointer-events-none"}`}
      aria-hidden={!open}
    >
      <button
        type="button"
        aria-label={`Close ${title}`}
        tabIndex={-1}
        onClick={onClose}
        className={`absolute inset-0 bg-black/65 transition-opacity ${open ? "opacity-100" : "opacity-0"}`}
      />
      <aside
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className={`mobile-side-menu absolute bottom-0 left-0 top-0 flex w-[min(82vw,21rem)] flex-col border-r-2 border-[#4a5180] bg-[#090d20]/[0.98] shadow-[8px_0_30px_rgba(0,0,0,0.55)] transition-transform duration-200 ${open ? "translate-x-0" : "-translate-x-full"}`}
      >
        <div className="flex min-h-16 items-center justify-between border-b border-[#343b68] px-4 pt-[var(--safe-top)]">
          <h2 id={titleId} className="font-display text-xs uppercase tracking-[0.14em] text-neon-cyan">
            {title}
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="flex size-11 items-center justify-center text-2xl text-white"
            aria-label={`Close ${title}`}
          >
            <span aria-hidden="true">{String.fromCharCode(215)}</span>
          </button>
        </div>
        <div className="flex-1 overflow-y-auto overscroll-contain p-4 pb-[max(1rem,var(--safe-bottom))]">
          {children}
        </div>
      </aside>
    </div>
  );
}
