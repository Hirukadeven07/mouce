import { percent, type Pull } from "./usePull";

// Progress bar, percentage (when known) and status text while a download runs, or the
// error word for word when it failed.
export function PullStatus(props: { pull: Pull }) {
  const { pull } = props;
  if (pull.phase === "error") return <p role="alert" className="error">{pull.message}</p>;
  if (pull.phase !== "running") return null;
  const pct = percent(pull.progress);
  return (
    <div className="progress">
      <progress aria-label="Download progress" max={100} value={pct === null ? undefined : pct} />
      {pct !== null && <span>{pct}%</span>}
      {pull.progress?.status && <span className="muted">{pull.progress.status}</span>}
    </div>
  );
}
