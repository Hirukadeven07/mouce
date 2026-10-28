import { useCallback, useEffect, useId, useRef, useState, type FormEvent } from "react";
import {
  clearApiKey,
  hasApiKey,
  listModels,
  ollamaStatus,
  openOllamaDownload,
  setApiKey,
  testProvider,
} from "../lib/ipc";
import type { ProviderId } from "../lib/types";
import { Switch } from "../ui/Switch";
import { PullStatus } from "./PullProgress";
import { usePull } from "./usePull";
import { errorText, useSettings } from "./useSettings";

export const DEFAULT_LOCAL_MODEL = "gemma3:4b";

export const PROVIDERS: Array<{ id: ProviderId; label: string }> = [
  { id: "ollama", label: "Local (Ollama)" },
  { id: "openai", label: "OpenAI" },
  { id: "groq", label: "Groq" },
  { id: "gemini", label: "Gemini" },
  { id: "anthropic", label: "Claude" },
];

// An error to show. `getOllama` adds the "Get Ollama" button for a missing install.
interface Problem {
  text: string;
  getOllama?: boolean;
}

type TestResult =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "ok"; text: string }
  | { kind: "error"; problem: Problem };

// When the local AI fails, find out whether Ollama is simply not running or not
// installed and say so; otherwise show the original error word for word.
async function explain(provider: ProviderId, err: unknown): Promise<Problem> {
  const original = { text: errorText(err) };
  if (provider !== "ollama") return original;
  try {
    const status = await ollamaStatus();
    if (status.state === "notRunning") {
      return { text: "Ollama isn't running — start it from the Start menu, then press Refresh" };
    }
    if (status.state === "notInstalled") {
      return { text: "Ollama isn't installed yet", getOllama: true };
    }
  } catch {
    // Fall through to the original error.
  }
  return original;
}

// Provider, model, API key and connection test. Shared by the Settings window
// and the first-run screen (`compact` drops the extra options).
export function AiSettings(props: { compact?: boolean }) {
  const { settings, error: settingsError, save } = useSettings();
  const { pull, pulling, start: startPull } = usePull();
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
  const [modelsLoaded, setModelsLoaded] = useState(false);
  const [modelsError, setModelsError] = useState<Problem | null>(null);
  const [openError, setOpenError] = useState<string | null>(null);
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
      if (id === modelRequest.current) {
        setModels(list);
        setModelsLoaded(true);
      }
    } catch (e) {
      const problem = await explain(p, e);
      if (id === modelRequest.current) {
        setModels([]);
        setModelsLoaded(false);
        setModelsError(problem);
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
    // Drop any model request still in flight for the previous provider.
    ++modelRequest.current;
    setModelsBusy(false);
    setModels([]);
    setModelsLoaded(false);
    setModelsError(null);
    setOpenError(null);
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
  // Only for Ollama, and only once the installed list is known.
  const notDownloaded = provider === "ollama" && modelsLoaded && !models.includes(savedModel);
  // The first-run screen has its own download step, so the compact form leaves it out.
  const showDownload = !props.compact && notDownloaded && savedModel !== "";
  const modelOptions = savedModel && !models.includes(savedModel) ? [savedModel, ...models] : models;
  const optionLabel = (m: string) => (m === savedModel && notDownloaded ? `${m} (not downloaded)` : m);

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
      setModelsLoaded(false);
      setModelsError(null);
      setTest({ kind: "idle" });
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
      const problem = await explain(p, err);
      if (current.current === p) setTest({ kind: "error", problem });
    }
  }

  async function onDownload() {
    const p = provider as ProviderId;
    if ((await startPull(savedModel)) && current.current === p) void loadModels(p);
  }

  async function onGetOllama() {
    setOpenError(null);
    try {
      await openOllamaDownload();
    } catch (err) {
      setOpenError(errorText(err));
    }
  }

  function renderProblem(problem: Problem) {
    return (
      <>
        <p role="alert" className="error">{problem.text}</p>
        {problem.getOllama && (
          <div className="button-row">
            <button type="button" onClick={() => void onGetOllama()}>
              Get Ollama (free, runs on this laptop)
            </button>
          </div>
        )}
        {problem.getOllama && openError && <p role="alert" className="error">{openError}</p>}
      </>
    );
  }

  return (
    <div className="ai-settings">
      <div className="group">
        <div className="form-row">
          <div className="form-line">
            <label className="form-label" htmlFor={ids.provider}>
              Provider
            </label>
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
        </div>

        {provider !== "ollama" && (
          <form className="form-row" onSubmit={onSaveKey}>
            <div className="form-line">
              <label className="form-label" htmlFor={ids.key}>
                API key
              </label>
              <input
                id={ids.key}
                type="password"
                autoComplete="off"
                spellCheck={false}
                value={keyInput}
                onChange={(e) => setKeyInput(e.target.value)}
              />
            </div>
            <div className="form-line">
              <span className="muted">
                {keySaved === null ? "" : keySaved ? "Key saved" : "No key yet"}
              </span>
              <div className="button-row">
                <button type="button" disabled={keyBusy || keySaved !== true} onClick={onRemoveKey}>
                  Remove key
                </button>
                <button type="submit" disabled={keyBusy || keyInput.trim() === ""}>
                  Save key
                </button>
              </div>
            </div>
            {keyError && <p role="alert" className="error">{keyError}</p>}
          </form>
        )}

        <div className="form-row">
          <div className="form-line">
            <label className="form-label" htmlFor={ids.model}>
              Model
            </label>
            <div className="form-control">
              <select
                id={ids.model}
                value={savedModel}
                onChange={(e) => void save({ models: { ...settings.models, [provider]: e.target.value } })}
              >
                {savedModel === "" && <option value="">Choose a model</option>}
                {modelOptions.map((m) => (
                  <option key={m} value={m}>
                    {optionLabel(m)}
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
          </div>
          {modelsError && renderProblem(modelsError)}
          {showDownload && (
            <div className="button-row">
              <button type="button" disabled={pulling} onClick={() => void onDownload()}>
                Download {savedModel}
              </button>
            </div>
          )}
          {(showDownload || pulling) && <PullStatus pull={pull} />}
        </div>

        <div className="form-row">
          <div className="form-line">
            <span className="form-label">
              {test.kind === "idle" && "Connection"}
              {test.kind === "running" && <span className="muted">Testing…</span>}
              {test.kind === "ok" && <span role="status" className="ok">Connected: {test.text}</span>}
              {test.kind === "error" && "Connection"}
            </span>
            <button type="button" disabled={test.kind === "running"} onClick={onTest}>
              Test connection
            </button>
          </div>
          {test.kind === "error" && renderProblem(test.problem)}
        </div>

        {!props.compact && (
          <div className="form-row">
            <div className="form-line">
              <label className="form-label" htmlFor={ids.fallback}>
                If the online AI fails, use local
              </label>
              <Switch
                id={ids.fallback}
                checked={settings.fallback_to_local}
                onChange={(e) => void save({ fallback_to_local: e.target.checked })}
              />
            </div>
          </div>
        )}
      </div>
      {settingsError && <p role="alert" className="error">{settingsError}</p>}
    </div>
  );
}
