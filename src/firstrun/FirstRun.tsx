import { useCallback, useEffect, useId, useRef, useState } from "react";
import { finishFirstRun, ollamaStatus, openOllamaDownload, setAutostart } from "../lib/ipc";
import type { OllamaStatus } from "../lib/types";
import { AiSettings, DEFAULT_LOCAL_MODEL } from "../settings/AiSettings";
import { PullStatus } from "../settings/PullProgress";
import { usePull } from "../settings/usePull";
import { errorText } from "../settings/useSettings";

function ChooseAi(props: { onNext: () => void }) {
  const [status, setStatus] = useState<OllamaStatus | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [online, setOnline] = useState(false);
  const { pull, pulling, start: startPull } = usePull();
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
      if (quiet) {
        // A background re-check failing must not undo what the screen already shows.
        console.error("Could not re-check Ollama", e);
      } else if (mounted.current && id === request.current) {
        setStatusError(errorText(e));
      }
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    void check(false);
    return () => {
      mounted.current = false;
    };
  }, [check]);

  async function download() {
    if (await startPull(DEFAULT_LOCAL_MODEL)) void check(true);
  }

  async function getOllama() {
    setOpenError(null);
    try {
      await openOllamaDownload();
    } catch (e) {
      if (mounted.current) setOpenError(errorText(e));
    }
  }

  const hasModel = status?.state === "running" && status.models.includes(DEFAULT_LOCAL_MODEL);
  // A finished download counts even if the follow-up status check could not run.
  const ready = pull.phase === "done" || hasModel;
  const needsModel = !ready && status?.state === "running";

  const onlineButton = (
    <button type="button" onClick={() => setOnline(true)} disabled={online}>
      Use an online AI
    </button>
  );

  return (
    <>
      <h1>Choose your AI</h1>

      {status === null && statusError === null && !ready && <p role="status">Checking for Ollama…</p>}
      {statusError && !ready && (
        <>
          <p role="alert" className="error">{statusError}</p>
          <div className="row">
            <button type="button" onClick={() => void check(false)}>Check again</button>
            {onlineButton}
          </div>
        </>
      )}

      {!ready && status?.state === "notInstalled" && (
        <>
          <p>mouce can use a free AI that runs on this laptop, or an online AI with your own key.</p>
          <div className="row">
            <button type="button" onClick={() => void getOllama()}>
              Get Ollama (free, runs on this laptop)
            </button>
            <button type="button" onClick={() => void check(false)}>Check again</button>
            {onlineButton}
          </div>
          {openError && <p role="alert" className="error">{openError}</p>}
        </>
      )}

      {!ready && status?.state === "notRunning" && (
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
          <p>Ollama is running. mouce needs one model to start with.</p>
          <div className="row">
            <button type="button" disabled={pulling} onClick={() => void download()}>
              Download {DEFAULT_LOCAL_MODEL} (about 3 GB)
            </button>
            {onlineButton}
          </div>
          <PullStatus pull={pull} />
        </>
      )}

      {ready && (
        <>
          <p role="status">{DEFAULT_LOCAL_MODEL} is ready.</p>
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
          <button type="button" className="primary" disabled={pulling} onClick={props.onNext}>
            {needsModel ? "Skip" : "Next"}
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
          <h1>Hi, I'm mouce.</h1>
          <p>
            I live in the corner of your desktop and help you understand what's on your
            screen. I can never open, change, delete or run anything on your computer.
          </p>
          <div className="check">
            <input
              id={startupId}
              type="checkbox"
              checked={autostart}
              onChange={(e) => setAutostartChoice(e.target.checked)}
            />
            <label htmlFor={startupId}>Start mouce when Windows starts</label>
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
          <p>Click mouce any time to open its settings.</p>
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
