import { useCallback, useEffect, useRef, useState } from "react";
import { onPullProgress, pullOllamaModel } from "../lib/ipc";
import type { PullProgress } from "../lib/types";
import { errorText } from "./useSettings";

export type Pull =
  | { phase: "idle" }
  | { phase: "running"; progress: PullProgress | null }
  | { phase: "done" }
  | { phase: "error"; message: string };

// Whole percent when the total size is known, otherwise null.
export function percent(p: PullProgress | null): number | null {
  if (!p || p.total === null || p.total <= 0 || p.completed === null) return null;
  return Math.max(0, Math.min(100, Math.round((p.completed / p.total) * 100)));
}

// Downloads an Ollama model and follows its progress events. `start` resolves to true
// when the download finished, false when it failed (the error is in `pull`).
export function usePull() {
  const [pull, setPull] = useState<Pull>({ phase: "idle" });
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    const unlisten = onPullProgress((progress) => {
      if (!mounted.current) return;
      setPull((cur) => (cur.phase === "running" ? { phase: "running", progress } : cur));
    }).catch(() => null);
    return () => {
      mounted.current = false;
      unlisten.then((un) => un?.()).catch(() => {});
    };
  }, []);

  const start = useCallback(async (model: string): Promise<boolean> => {
    setPull({ phase: "running", progress: null });
    try {
      await pullOllamaModel(model);
      if (mounted.current) setPull({ phase: "done" });
      return mounted.current;
    } catch (e) {
      if (mounted.current) setPull({ phase: "error", message: errorText(e) });
      return false;
    }
  }, []);

  const pulling = pull.phase === "running";
  const pct = pull.phase === "running" ? percent(pull.progress) : null;
  return { pull, pulling, pct, start };
}
