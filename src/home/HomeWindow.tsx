import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import logoUrl from "../assets/mouce.png";
import { initialHomeTab, onHomeTab } from "../lib/ipc";
import type { HomeTabId } from "../lib/types";
import { Settings } from "../settings/Settings";
import { useSettings } from "../settings/useSettings";
import { CalendarTab, RemindersTab } from "./CalendarTab";
import { Customize } from "./Customize";
import { greeting, longDate } from "./dates";
import { HomeTab } from "./HomeTab";

const TABS: Array<{ id: HomeTabId; label: string }> = [
  { id: "home", label: "Home" },
  { id: "calendar", label: "Calendar" },
  { id: "reminders", label: "Reminders" },
  { id: "customize", label: "Customize" },
  { id: "settings", label: "Settings" },
];

function isTab(value: unknown): value is HomeTabId {
  return TABS.some((t) => t.id === value);
}

/** Keeps the date current so the greeting and "today" roll over while the window is open. */
function useNow(): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 60_000);
    return () => window.clearInterval(id);
  }, []);
  return now;
}

export function HomeWindow() {
  const [tab, setTab] = useState<HomeTabId>(() => {
    const start = initialHomeTab();
    return isTab(start) ? start : "home";
  });
  const { settings } = useSettings();
  const now = useNow();
  const tabRefs = useRef<Partial<Record<HomeTabId, HTMLButtonElement | null>>>({});

  useEffect(() => {
    let alive = true;
    const unlisten = onHomeTab((t) => alive && isTab(t) && setTab(t)).catch(() => null);
    return () => {
      alive = false;
      unlisten.then((un) => un?.()).catch(() => {});
    };
  }, []);

  // Arrow keys move between tabs, as in any tab list.
  function onTabKey(e: KeyboardEvent<HTMLButtonElement>) {
    const i = TABS.findIndex((t) => t.id === tab);
    let next: number | null = null;
    if (e.key === "ArrowRight") next = (i + 1) % TABS.length;
    if (e.key === "ArrowLeft") next = (i - 1 + TABS.length) % TABS.length;
    if (e.key === "Home") next = 0;
    if (e.key === "End") next = TABS.length - 1;
    if (next === null) return;
    e.preventDefault();
    const id = TABS[next].id;
    setTab(id);
    tabRefs.current[id]?.focus();
  }

  return (
    <div className="home">
      <header className="home-header">
        <img className="home-logo" src={logoUrl} alt="" width={36} height={36} />
        <h1>mouce</h1>
        <p className="home-greeting">
          {greeting(now)} · {longDate(now)}
        </p>
      </header>
      <div className="home-tabs" role="tablist" aria-label="mouce">
        {TABS.map((t) => (
          <button
            key={t.id}
            ref={(el) => {
              tabRefs.current[t.id] = el;
            }}
            type="button"
            role="tab"
            id={`tab-${t.id}`}
            aria-selected={tab === t.id}
            aria-controls={`panel-${t.id}`}
            tabIndex={tab === t.id ? 0 : -1}
            className={"home-tab" + (t.id === "settings" ? " home-tab--end" : "")}
            onClick={() => setTab(t.id)}
            onKeyDown={onTabKey}
          >
            {t.id === "settings" && (
              <span className="home-tab-icon" aria-hidden="true">
                ⚙
              </span>
            )}
            {t.label}
          </button>
        ))}
      </div>
      <main className="home-panel" role="tabpanel" id={`panel-${tab}`} aria-labelledby={`tab-${tab}`}>
        {tab === "home" && <HomeTab settings={settings} today={now} onNavigate={setTab} />}
        {tab === "calendar" && <CalendarTab today={now} />}
        {tab === "reminders" && <RemindersTab />}
        {tab === "customize" && <Customize />}
        {tab === "settings" && <Settings />}
      </main>
    </div>
  );
}
