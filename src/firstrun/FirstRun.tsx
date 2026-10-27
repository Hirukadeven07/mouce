import { useCallback, useEffect, useId, useRef, useState } from "react";
import {
  finishFirstRun,
  onPullProgress,
  ollamaStatus,
  openOllamaDownload,
  pullOllamaModel,
  setAutostart,
} from "../lib/ipc";
import type { OllamaStatus, PullProgress } from "../lib/types";
import { AiSettings, DEFAULT_LOCAL_MODEL } from "../settings/AiSettings";
import { errorText } from "../settings/useSettings";

type Pull =
  | { phase: "idle" }
  | { phase: "running"; progress: PullProgress | null }
  | { phase: "done" }
  | { phase: "error"; message: string };

function percent(p: PullProgress | null): number | null {
  if (!p || p.total === null || p.total <= 0 || p.completed === null) return null;
  return Math.max(0, Math.min(100, Math.round((p.completed / p.total) * 100)));
}

function ChooseAi(props: { onNext: () => void }) {
  const [status, setStatus] = useState<OllamaStatus | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [online, setOnline] = useState(false);
  const [pull, setPull] = useState<Pull>({ phase: "idle" });
  const [openError, setOpenError] = useState<string | null>(null);
  const request = useRef(0);
  const mounted = useRef(true);

  const check = useCallback(async (quiet: boolean) => {
    const id = ++request.current;
    if (!quiet) setStatus(null);
    setStatusError(null);
    try {
      const s = await ollamaStatus();
      if (mounted.current && id === request.current) setStatus(s);
    } catch (e) {
      if (mounted.current && id === request.current) setStatusError(errorText(e));
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    void check(false);
    const unlisten = onPullProgress((progress) => {
      if (!mounted.current) return;
      setPull((cur) => (cur.phase === "running" ? { phase: "running", progress } : cur));
    }).catch(() => null);
    return () => {
      mounted.current = false;
      unlisten.then((un) => un?.()).catch(() => {});
    };
  }, [check]);

  async function download() {
    setPull({ phase: "running", progress: null });
    try {
      await pullOllamaModel(DEFAULT_LOCAL_MODEL);
      if (!mounted.current) return;
      setPull({ phase: "done" });
      void check(true);
    } catch (e) {
      if (mounted.current) setPull({ phase: "error", message: errorText(e) });
    }
  }

  async function getOllama() {
    setOpenError(null);
    try {
      await openOllamaDownload();
    } catch (e) {
      if (mounted.current) setOpenError(errorText(e));
    }
  }

  const pct = pull.phase === "running" ? percent(pull.progress) : null;
  const pulling = pull.phase === "running";
  const needsModel =
    status?.state === "running" && !status.models.includes(DEFAULT_LOCAL_MODEL);
  const ready = status?.state === "running" && !needsModel;

  const onlineButton = (
    <button type="button" onClick={() => setOnline(true)} disabled={online}>
      Use an online AI
    </button>
  );

  return (
    <>
      <h1>Choose your AI</h1>

      {status === null && statusError === null && <p role="status">Checking for Ollama…</p>}
      {statusError && (
        <>
          <p role="alert" className="error">{statusError}</p>
          <div className="row">
            <button type="button" onClick={() => void check(false)}>Check again</button>
            {onlineButton}
          </div>
        </>
      )}

      {status?.state === "notInstalled" && (
        <>
          <p>Star can use a free AI that runs on this laptop, or an online AI with your own key.</p>
          <div className="row">
            <button type="button" onClick={() => void getOllama()}>
              Get Ollama (free, runs on this laptop)
            </button>
            {onlineButton}
          </div>
          {openError && <p role="alert" className="error">{openError}</p>}
        </>
      )}

      {status?.state === "notRunning" && (
        <>
          <p>
            Ollama is installed but not running — start it from the Start menu, then press Check
            again
          </p>
          <div className="row">
            <button type="button" onClick={() => void check(false)}>Check again</button>
            {onlineButton}
          </div>
        </>
      )}

      {needsModel && (
        <>
          <p>Ollama is running. Star needs one model to start with.</p>
          <div className="row">
            <button type="button" disabled={pulling} onClick={() => void download()}>
              Download {DEFAULT_LOCAL_MODEL} (about 3 GB)
            </button>
            {onlineButton}
          </div>
          {pull.phase === "running" && (
            <div className="progress">
              <progress
                aria-label="Download progress"
                max={100}
                value={pct === null ? undefined : pct}
              />
              {pct !== null && <span>{pct}%</span>}
              {pull.progress?.status && <span className="muted">{pull.progress.status}</span>}
            </div>
          )}
          {pull.phase === "error" && <p role="alert" className="error">{pull.message}</p>}
        </>
      )}

      {ready && (
        <>
          <p role="status">Local AI is ready ({DEFAULT_LOCAL_MODEL}).</p>
          <div className="row">{onlineButton}</div>
        </>
      )}

      {online && (
        <section>
          <AiSettings compact />
        </section>
      )}

      {(status !== null || statusError !== null) && (
        <div className="footer">
          <button type="button" className="primary" onClick={props.onNext}>
            {needsModel && !pulling ? "Skip" : "Next"}
          </button>
        </div>
      )}
    </>
  );
}

export function FirstRun() {
  const [step, setStep] = useState<1 | 2 | 3>(1);
  const [autostart, setAutostartChoice] = useState(true);
  const [finishing, setFinishing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const skipAutostart = useRef(false);
  const startupId = useId();

  async function done() {
    setFinishing(true);
    setError(null);
    try {
      if (!skipAutostart.current) {
        try {
          await setAutostart(autostart);
        } catch (e) {
          skipAutostart.current = true;
          setError(
            `Couldn't change the startup setting: ${errorText(e)}. Press Done again to finish without it; you can change it later in Settings.`,
          );
          return;
        }
      }
      await finishFirstRun();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setFinishing(false);
    }
  }

  return (
    <main className="page firstrun">
      {step === 1 && (
        <>
          <h1>Hi, I'm Star.</h1>
          <p>
            I'm a tiny star that lives on your desktop and helps you understand what's on your
            screen. I can never open, change, delete or run anything on your computer.
          </p>
          <div className="check">
            <input
              id={startupId}
              type="checkbox"
              checked={autostart}
              onChange={(e) => setAutostartChoice(e.target.checked)}
            />
            <label htmlFor={startupId}>Start Star when Windows starts</label>
          </div>
          <div className="footer">
            <button type="button" className="primary" onClick={() => setStep(2)}>
              Next
            </button>
          </div>
        </>
      )}

      {step === 2 && <ChooseAi onNext={() => setStep(3)} />}

      {step === 3 && (
        <>
          <h1>All set</h1>
          <p>Click the star any time to open its settings.</p>
          {error && <p role="alert" className="error">{error}</p>}
          <div className="footer">
            <button type="button" className="primary" disabled={finishing} onClick={() => void done()}>
              Done
            </button>
          </div>
        </>
      )}
    </main>
  );
}
