import { openUrl } from "@tauri-apps/plugin-opener";
import { isRunningInTauri } from "../../lib/tauri";
import { safePolicyUrl } from "./model";
import { useFeedbackStore } from "./store";
import type { FeedbackProgramDocument } from "./types";

export function FeedbackPolicyLinks({ document }: { document: FeedbackProgramDocument }) {
  return (
    <div className="flex flex-wrap gap-x-4 gap-y-1">
      {[["Privacy Policy", document.privacyUrl], ["Terms of Use", document.termsUrl], ["Deletion help", document.deletionUrl]].map(([label, value]) => {
        const url = safePolicyUrl(value);
        if (!url) return null;
        return <a key={label} href={url} target="_blank" rel="noopener noreferrer" className="inline-flex min-h-11 items-center text-neon-cyan underline underline-offset-4" onClick={(event) => {
          if (!isRunningInTauri()) return;
          event.preventDefault();
          void openUrl(url).catch(() => useFeedbackStore.getState().notify("The policy link could not be opened. Please try again."));
        }}>{label}<span className="sr-only"> (opens externally)</span></a>;
      })}
    </div>
  );
}
