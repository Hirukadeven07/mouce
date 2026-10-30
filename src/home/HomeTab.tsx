import type { HomeTabId, Settings } from "../lib/types";
import { PROVIDERS } from "../settings/AiSettings";
import { addDays, gridRange, isoDate, monthTitle } from "./dates";
import { ItemList } from "./ItemList";
import { MonthView } from "./MonthView";
import { byDate, useCalendarRange, useReminders } from "./useCalendar";
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

/** How many reminders the dashboard lists. */
const SHOWN_REMINDERS = 3;
/** How many days "Coming up" looks ahead, today included, and how many events it lists. */
const COMING_DAYS = 7;
const SHOWN_COMING = 5;

/** The dashboard: the AI in use, this month, the week's events and open reminders. */
export function HomeTab(props: {
  settings: Settings | null;
  today: Date;
  onNavigate: (tab: HomeTabId) => void;
}) {
  const { settings, today, onNavigate } = props;
  const year = today.getFullYear();
  const month = today.getMonth();
  const { from, to } = gridRange(year, month, today);
  const days = byDate(useCalendarRange(from, to));
  const reminders = useReminders();
  const open = reminders && reminders.filter((r) => !r.done);
  const start = isoDate(today);
  // Reminders have their own card, so this one is the events.
  const coming = useCalendarRange(start, addDays(start, COMING_DAYS - 1));
  const events = coming && coming.filter((i) => i.kind === "event");
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
          <MonthView year={year} month={month} today={today} marked={new Set(days.keys())} />
        </section>
        <div className="home-stack">
          <section className="card" aria-labelledby="coming-card-title">
            <div className="card-head">
              <h2 id="coming-card-title" className="headline">
                Coming up
              </h2>
              {events && events.length > SHOWN_COMING && (
                <button type="button" className="link" onClick={() => onNavigate("calendar")}>
                  {events.length - SHOWN_COMING} more
                </button>
              )}
            </div>
            <ItemList
              items={events && events.slice(0, SHOWN_COMING)}
              now={today}
              withDay
              empty="Nothing planned this week."
              label="Coming up"
            />
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
            <ItemList
              items={open && open.slice(0, SHOWN_REMINDERS)}
              now={today}
              withDay
              empty={reminders?.length ? "Nothing left to do." : "No reminders yet."}
              label="Reminders"
            />
          </section>
        </div>
      </div>
    </div>
  );
}
