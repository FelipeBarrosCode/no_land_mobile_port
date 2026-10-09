import { useEffect, type ReactNode } from "react";

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
  useEffect(() => {
    if (!open) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose, open]);

  return (
    <div
      className={`fixed inset-0 z-[90] md:hidden ${open ? "pointer-events-auto" : "pointer-events-none"}`}
      aria-hidden={!open}
    >
      <button
        type="button"
        aria-label={title}
        tabIndex={open ? 0 : -1}
        onClick={onClose}
        className={`absolute inset-0 bg-black/65 transition-opacity ${open ? "opacity-100" : "opacity-0"}`}
      />
      <aside
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className={`mobile-side-menu absolute bottom-0 left-0 top-0 flex w-[min(82vw,21rem)] flex-col border-r-2 border-[#4a5180] bg-[#090d20]/[0.98] shadow-[8px_0_30px_rgba(0,0,0,0.55)] transition-transform duration-200 ${open ? "translate-x-0" : "-translate-x-full"}`}
      >
        <div className="flex min-h-16 items-center justify-between border-b border-[#343b68] px-4 pt-[var(--safe-top)]">
          <h2 className="font-display text-xs uppercase tracking-[0.14em] text-neon-cyan">
            {title}
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="flex size-11 items-center justify-center text-2xl text-white"
            aria-label={title}
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
