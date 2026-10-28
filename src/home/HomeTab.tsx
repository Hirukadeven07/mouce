import type { HomeTabId, Settings } from "../lib/types";
import { PROVIDERS } from "../settings/AiSettings";
import { monthTitle } from "./dates";
import { MonthView } from "./MonthView";
import { activeModel, useAiStatus } from "./useAiStatus";

function providerPill(settings: Settings): string {
  if (settings.provider === "ollama") return "Local · Ollama";
  const label = PROVIDERS.find((p) => p.id === settings.provider)?.label ?? settings.provider;
  return `Online · ${label}`;
}

function AiCard(props: { settings: Settings; onChange: () => void }) {
  const { settings, onChange } = props;
  const status = useAiStatus(settings);
  const model = activeModel(settings);
  const tone = status.kind === "ready" ? "ok" : status.kind === "problem" ? "warn" : "checking";
  return (
    <section className="card ai-card" aria-labelledby="ai-card-title">
      <span className={"ai-dot ai-dot--" + tone} aria-hidden="true" />
      <div className="ai-text">
        <h2 id="ai-card-title" className="ai-model">
          {model ?? "No model chosen"}
        </h2>
        <p className={"ai-status ai-status--" + tone} role="status">
          {status.kind === "checking" ? "Checking…" : status.text}
        </p>
      </div>
      <span className="pill">{providerPill(settings)}</span>
      <button type="button" className="ai-change" onClick={onChange}>
        Change AI
      </button>
    </section>
  );
}

/** The dashboard: the AI in use, this month, today's events and open reminders. */
export function HomeTab(props: {
  settings: Settings | null;
  today: Date;
  onNavigate: (tab: HomeTabId) => void;
}) {
  const { settings, today, onNavigate } = props;
  const year = today.getFullYear();
  const month = today.getMonth();
  return (
    <div className="home-dash">
      {settings ? (
        <AiCard settings={settings} onChange={() => onNavigate("settings")} />
      ) : (
        <section className="card ai-card">
          <p role="status">Loading…</p>
        </section>
      )}
      <div className="home-grid">
        <section className="card" aria-labelledby="month-card-title">
          <div className="card-head">
            <h2 id="month-card-title" className="headline">
              {monthTitle(year, month)}
            </h2>
            <button type="button" className="link" onClick={() => onNavigate("calendar")}>
              Open calendar
            </button>
          </div>
          <MonthView year={year} month={month} today={today} />
        </section>
        <div className="home-stack">
          <section className="card" aria-labelledby="today-card-title">
            <div className="card-head">
              <h2 id="today-card-title" className="headline">
                Today
              </h2>
            </div>
            <p className="empty">Nothing planned today.</p>
          </section>
          <section className="card" aria-labelledby="reminders-card-title">
            <div className="card-head">
              <h2 id="reminders-card-title" className="headline">
                Reminders
              </h2>
              <button type="button" className="link" onClick={() => onNavigate("reminders")}>
                See all
              </button>
            </div>
            <p className="empty">No reminders yet.</p>
          </section>
        </div>
      </div>
    </div>
  );
}
