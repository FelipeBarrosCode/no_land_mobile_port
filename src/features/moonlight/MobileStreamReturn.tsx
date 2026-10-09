import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Button } from "../../components/ui/Button";
import {
  moonlightDisconnectStream,
  moonlightGetClipboardFromRemote,
  moonlightPresentStream,
  moonlightSendClipboardToRemote,
} from "../../lib/backend";
import { translate, translateSource } from "../../lib/i18n";

export function MobileStreamReturn() {
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [action, setAction] = useState<"send" | "get" | "disconnect" | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [streamStats, setStreamStats] = useState<{ renderedFpsX100: number; averageDecodePipelineUs: number } | null>(null);

  useEffect(() => {
    const unlisten = listen<{ renderedFpsX100: number; averageDecodePipelineUs: number }>(
      "moonlight://statistics",
      ({ payload }) => setStreamStats(payload),
    );
    return () => { void unlisten.then((dispose) => dispose()); };
  }, []);
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
  async function clipboard(direction: "send" | "get") {
    setAction(direction);
    setError(null);
    try {
      const result = direction === "send"
        ? await moonlightSendClipboardToRemote()
        : await moonlightGetClipboardFromRemote();
      setStatus(translate(direction === "send" ? "stream.clipboard.sent" : "stream.clipboard.received", {
        count: result.byteCount,
      }));
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setAction(null);
    }
  }
  async function disconnect() {
    setAction("disconnect");
    setError(null);
    try {
      await moonlightDisconnectStream();
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
      setAction(null);
    }
  }
  return (
    <div className="sticky top-0 z-40 border-b border-[#3e4270] bg-[#10122af2] p-3 backdrop-blur">
      <div className="flex flex-wrap gap-2">
        <Button onClick={() => void returnToStream()} disabled={pending || action !== null}>
          {translate("mobile.returnToStream")}
        </Button>
        <Button variant="ghost" onClick={() => void clipboard("send")} disabled={action !== null}>
          {translate("generated.639d984d9ffc0361")}
        </Button>
        <Button variant="ghost" onClick={() => void clipboard("get")} disabled={action !== null}>
          {translate("generated.9d060e4dd135b141")}
        </Button>
        <Button variant="danger" onClick={() => void disconnect()} disabled={action !== null}>
          {action === "disconnect" ? translate("generated.6feadc6d3b71fbcc") : translate("generated.23c13af33709b5e3")}
        </Button>
      </div>
      {streamStats && (
        <p className="mt-2 text-sm text-[#9fb3cd]">
          {translate("mobile.streamStats", {
            fps: (streamStats.renderedFpsX100 / 100).toFixed(1),
            decode: (streamStats.averageDecodePipelineUs / 1000).toFixed(2),
          })}
        </p>
      )}
      {status && <p role="status" className="mt-2 text-[#7cff47]">{status}</p>}
      {error && <p role="alert" className="mt-2 text-[#ff687d]">{translateSource(error)}</p>}
    </div>
  );
}
