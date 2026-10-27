import { useCallback, useEffect, useRef, useState } from "react";
import { getSettings, onSettingsChanged, updateSettings } from "../lib/ipc";
import type { Settings } from "../lib/types";

// Tauri rejects with the Rust error string; other failures are Error objects.
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

// Loads the settings, keeps them in step with changes made in other windows or
// components, and saves a partial change on top of the latest known settings.
// The backend returns what it actually saved; that becomes the new state.
export function useSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const latest = useRef<Settings | null>(null);
  const seq = useRef(0);
  const mounted = useRef(true);

  const apply = useCallback((s: Settings) => {
    latest.current = s;
    setSettings(s);
  }, []);

  useEffect(() => {
    mounted.current = true;
    const unlisten = onSettingsChanged((s) => mounted.current && apply(s)).catch(() => null);
    getSettings()
      .then((s) => {
        if (mounted.current && latest.current === null) apply(s);
      })
      .catch((e) => mounted.current && setError(errorText(e)));
    return () => {
      mounted.current = false;
      unlisten.then((un) => un?.()).catch(() => {});
    };
  }, [apply]);

  const save = useCallback(
    async (patch: Partial<Settings>) => {
      const base = latest.current;
      if (!base) return;
      const next = { ...base, ...patch };
      const id = ++seq.current;
      apply(next);
      setError(null);
      try {
        const saved = await updateSettings(next);
        if (mounted.current && id === seq.current) apply(saved);
      } catch (e) {
        if (!mounted.current || id !== seq.current) return;
        setError(errorText(e));
        try {
          apply(await getSettings());
        } catch {
          // Keep showing what we have; the error above says the save failed.
        }
      }
    },
    [apply],
  );

  return { settings, error, save };
}
