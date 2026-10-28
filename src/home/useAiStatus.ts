import { useEffect, useState } from "react";
import { hasApiKey, ollamaStatus } from "../lib/ipc";
import type { Settings } from "../lib/types";
import { DEFAULT_LOCAL_MODEL } from "../settings/AiSettings";
import { errorText } from "../settings/useSettings";

export type AiStatus =
  | { kind: "checking" }
  | { kind: "ready"; text: string }
  | { kind: "problem"; text: string };

/** The model the current provider will use, or null if none is chosen yet. */
export function activeModel(settings: Settings): string | null {
  const saved = settings.models[settings.provider];
  if (saved) return saved;
  return settings.provider === "ollama" ? DEFAULT_LOCAL_MODEL : null;
}

async function check(settings: Settings): Promise<AiStatus> {
  const model = activeModel(settings);
  if (settings.provider === "ollama") {
    const status = await ollamaStatus();
    if (status.state === "notInstalled") return { kind: "problem", text: "Ollama isn't installed" };
    if (status.state === "notRunning") {
      return { kind: "problem", text: "Ollama isn't running. Start it from the Start menu." };
    }
    if (!model || !status.models.includes(model)) {
      return { kind: "problem", text: "This model isn't downloaded yet" };
    }
    return { kind: "ready", text: "Ready · runs on this laptop" };
  }
  if (!(await hasApiKey(settings.provider))) {
    return { kind: "problem", text: "Add an API key in Settings" };
  }
  if (!model) return { kind: "problem", text: "Choose a model in Settings" };
  return { kind: "ready", text: "Ready · online with your key" };
}

/**
 * Whether the chosen AI can answer right now. Checked again when the provider or model
 * changes and whenever the window regains focus (say, after starting Ollama).
 */
export function useAiStatus(settings: Settings | null): AiStatus {
  const [status, setStatus] = useState<AiStatus>({ kind: "checking" });
  const provider = settings?.provider;
  const model = settings ? activeModel(settings) : null;

  useEffect(() => {
    if (!settings) return;
    let current = true;
    const run = () => {
      check(settings)
        .then((s) => current && setStatus(s))
        .catch((e) => current && setStatus({ kind: "problem", text: errorText(e) }));
    };
    setStatus({ kind: "checking" });
    run();
    window.addEventListener("focus", run);
    return () => {
      current = false;
      window.removeEventListener("focus", run);
    };
    // Only the provider and model matter; other settings changes must not re-check.
  }, [provider, model]);

  return status;
}
