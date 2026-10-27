import { useCallback, useEffect, useId, useRef, useState, type FormEvent } from "react";
import { clearApiKey, hasApiKey, listModels, setApiKey, testProvider } from "../lib/ipc";
import type { ProviderId } from "../lib/types";
import { errorText, useSettings } from "./useSettings";

export const DEFAULT_LOCAL_MODEL = "gemma3:4b";

const PROVIDERS: Array<{ id: ProviderId; label: string }> = [
  { id: "ollama", label: "Local (Ollama)" },
  { id: "openai", label: "OpenAI" },
  { id: "groq", label: "Groq" },
  { id: "gemini", label: "Gemini" },
  { id: "anthropic", label: "Claude" },
];

type TestResult =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "ok"; text: string }
  | { kind: "error"; text: string };

// Provider, model, API key and connection test. Shared by the Settings window
// and the first-run screen (`compact` drops the extra options).
export function AiSettings(props: { compact?: boolean }) {
  const { settings, error: settingsError, save } = useSettings();
  const provider = settings?.provider;
  const ids = {
    provider: useId(),
    model: useId(),
    key: useId(),
    fallback: useId(),
  };

  const [keySaved, setKeySaved] = useState<boolean | null>(null);
  const [keyInput, setKeyInput] = useState("");
  const [keyBusy, setKeyBusy] = useState(false);
  const [keyError, setKeyError] = useState<string | null>(null);
  const [models, setModels] = useState<string[]>([]);
  const [modelsBusy, setModelsBusy] = useState(false);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [test, setTest] = useState<TestResult>({ kind: "idle" });

  // Always the provider on screen, so a slow answer for a provider the user has
  // already moved away from is dropped instead of shown.
  const current = useRef<ProviderId | undefined>(undefined);
  current.current = provider;
  const modelRequest = useRef(0);

  const loadModels = useCallback(async (p: ProviderId) => {
    const id = ++modelRequest.current;
    setModelsBusy(true);
    setModelsError(null);
    try {
      const list = await listModels(p);
      if (id === modelRequest.current) setModels(list);
    } catch (e) {
      if (id === modelRequest.current) {
        setModels([]);
        setModelsError(errorText(e));
      }
    } finally {
      if (id === modelRequest.current) setModelsBusy(false);
    }
  }, []);

  useEffect(() => {
    if (!provider) return;
    let cancelled = false;
    setKeyInput("");
    setKeyError(null);
    setTest({ kind: "idle" });
    setModels([]);
    setModelsError(null);
    setKeySaved(null);
    if (provider === "ollama") {
      void loadModels(provider);
    } else {
      hasApiKey(provider)
        .then((has) => {
          if (cancelled) return;
          setKeySaved(has);
          if (has) void loadModels(provider);
        })
        .catch((e) => {
          if (!cancelled) setKeyError(errorText(e));
        });
    }
    return () => {
      cancelled = true;
    };
  }, [provider, loadModels]);

  if (!settings || !provider) {
    return <p role="status">{settingsError ?? "Loading…"}</p>;
  }

  const savedModel = settings.models[provider] ?? (provider === "ollama" ? DEFAULT_LOCAL_MODEL : "");
  const modelOptions = savedModel && !models.includes(savedModel) ? [savedModel, ...models] : models;

  async function onSaveKey(e: FormEvent) {
    e.preventDefault();
    const p = provider as ProviderId;
    const key = keyInput.trim();
    if (!key) return;
    setKeyBusy(true);
    setKeyError(null);
    try {
      await setApiKey(p, key);
      if (current.current !== p) return;
      setKeyInput("");
      setKeySaved(true);
      void loadModels(p);
    } catch (err) {
      if (current.current === p) setKeyError(errorText(err));
    } finally {
      setKeyBusy(false);
    }
  }

  async function onRemoveKey() {
    const p = provider as ProviderId;
    setKeyBusy(true);
    setKeyError(null);
    try {
      await clearApiKey(p);
      if (current.current !== p) return;
      setKeyInput("");
      setKeySaved(false);
      setModels([]);
      setModelsError(null);
    } catch (err) {
      if (current.current === p) setKeyError(errorText(err));
    } finally {
      setKeyBusy(false);
    }
  }

  async function onTest() {
    const p = provider as ProviderId;
    setTest({ kind: "running" });
    try {
      const reply = await testProvider(p);
      if (current.current === p) setTest({ kind: "ok", text: reply });
    } catch (err) {
      if (current.current === p) setTest({ kind: "error", text: errorText(err) });
    }
  }

  return (
    <div className="ai-settings">
      <div className="field">
        <label htmlFor={ids.provider}>Provider</label>
        <select
          id={ids.provider}
          value={provider}
          onChange={(e) => void save({ provider: e.target.value as ProviderId })}
        >
          {PROVIDERS.map((p) => (
            <option key={p.id} value={p.id}>
              {p.label}
            </option>
          ))}
        </select>
      </div>

      {provider !== "ollama" && (
        <form className="field" onSubmit={onSaveKey}>
          <label htmlFor={ids.key}>API key</label>
          <input
            id={ids.key}
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={keyInput}
            onChange={(e) => setKeyInput(e.target.value)}
          />
          <div className="row">
            <button type="submit" disabled={keyBusy || keyInput.trim() === ""}>
              Save key
            </button>
            <button type="button" disabled={keyBusy || keySaved !== true} onClick={onRemoveKey}>
              Remove key
            </button>
            <span className="muted">
              {keySaved === null ? "" : keySaved ? "Key saved" : "No key yet"}
            </span>
          </div>
          {keyError && <p role="alert" className="error">{keyError}</p>}
        </form>
      )}

      <div className="field">
        <label htmlFor={ids.model}>Model</label>
        <div className="row">
          <select
            id={ids.model}
            value={savedModel}
            onChange={(e) => void save({ models: { ...settings.models, [provider]: e.target.value } })}
          >
            {savedModel === "" && <option value="">Choose a model</option>}
            {modelOptions.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
          <button
            type="button"
            disabled={modelsBusy || (provider !== "ollama" && keySaved !== true)}
            onClick={() => void loadModels(provider)}
          >
            Refresh
          </button>
        </div>
        {modelsError && <p role="alert" className="error">{modelsError}</p>}
      </div>

      <div className="field">
        <div className="row">
          <button type="button" disabled={test.kind === "running"} onClick={onTest}>
            Test connection
          </button>
          {test.kind === "running" && <span className="muted">Testing…</span>}
          {test.kind === "ok" && <span role="status" className="ok">Connected: {test.text}</span>}
        </div>
        {test.kind === "error" && <p role="alert" className="error">{test.text}</p>}
      </div>

      {!props.compact && (
        <div className="check">
          <input
            id={ids.fallback}
            type="checkbox"
            checked={settings.fallback_to_local}
            onChange={(e) => void save({ fallback_to_local: e.target.checked })}
          />
          <label htmlFor={ids.fallback}>If the online AI fails, use local</label>
        </div>
      )}
      {settingsError && <p role="alert" className="error">{settingsError}</p>}
    </div>
  );
}
