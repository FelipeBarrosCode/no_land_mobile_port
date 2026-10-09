import { useState } from "react";
import { Button } from "../../components/ui/Button";
import { moonlightPresentStream } from "../../lib/backend";
import { translate, translateSource } from "../../lib/i18n";

export function MobileStreamReturn() {
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  async function returnToStream() {
    setPending(true);
    setError(null);
    try {
      await moonlightPresentStream();
    } catch (failure) {
      const details = failure as { message?: string; details?: string };
      setError(details?.details || details?.message || String(failure));
    } finally {
      setPending(false);
    }
  }
  return (
    <div className="sticky top-0 z-40 border-b border-[#3e4270] bg-[#10122a] p-3">
      <Button onClick={() => void returnToStream()} disabled={pending}>
        {translate("mobile.returnToStream")}
      </Button>
      {error && <p role="alert" className="mt-2 text-[#ff687d]">{translateSource(error)}</p>}
    </div>
  );
}
