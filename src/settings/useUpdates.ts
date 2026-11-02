import { useCallback, useEffect, useRef, useState } from "react";
import {
  appVersion,
  onUpdateAvailable,
  updateCheck,
  updateCurrent,
  updateOpenDownload,
} from "../lib/ipc";
import type { UpdateInfo } from "../lib/types";
import { errorText } from "./useSettings";

export type UpdatePhase =
  | { phase: "idle" }
  | { phase: "checking" }
  | { phase: "latest" }
  | { phase: "error"; message: string };

// The running version, the newer one a check found (here or in the background), and the
// actions to look again or open the download page. mouce never installs anything itself.
export function useUpdates() {
  const [version, setVersion] = useState<string | null>(null);
  const [found, setFound] = useState<UpdateInfo | null>(null);
  const [state, setState] = useState<UpdatePhase>({ phase: "idle" });
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    Promise.resolve()
      .then(() => appVersion())
      .then((v) => mounted.current && v && setVersion(v))
      .catch(() => {});
    Promise.resolve()
      .then(() => updateCurrent())
      .then((u) => mounted.current && u && setFound(u))
      .catch(() => {});
    const unlisten = Promise.resolve()
      .then(() => onUpdateAvailable((u) => mounted.current && setFound(u)))
      .catch(() => null);
    return () => {
      mounted.current = false;
      unlisten.then((un) => un?.()).catch(() => {});
    };
  }, []);

  const check = useCallback(async () => {
    setState({ phase: "checking" });
    try {
      const u = await updateCheck();
      if (!mounted.current) return;
      setFound(u ?? null);
      setState(u ? { phase: "idle" } : { phase: "latest" });
    } catch (e) {
      if (mounted.current) setState({ phase: "error", message: errorText(e) });
    }
  }, []);

  const download = useCallback(async () => {
    try {
      await updateOpenDownload();
    } catch (e) {
      if (mounted.current) setState({ phase: "error", message: errorText(e) });
    }
  }, []);

  return { version, found, state, check, download };
}
