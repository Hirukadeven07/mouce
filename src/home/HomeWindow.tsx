import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { initialHomeTab, onHomeTab, openChat } from "../lib/ipc";
import type { HomeTabId } from "../lib/types";
import { Settings } from "../settings/Settings";
import { useSettings } from "../settings/useSettings";
import { Icon, type IconName } from "../ui/Icon";
import { CalendarTab, RemindersTab } from "./CalendarTab";
import { Customize } from "./Customize";
import { greeting, longDate } from "./dates";
import { HomeTab } from "./HomeTab";

const TABS: Array<{ id: HomeTabId; label: string; icon: IconName }> = [
  { id: "home", label: "Home", icon: "home" },
  { id: "calendar", label: "Calendar", icon: "calendar" },
  { id: "reminders", label: "Reminders", icon: "reminders" },
  { id: "customize", label: "Customize", icon: "customize" },
  { id: "settings", label: "Settings", icon: "settings" },
];

/** Where the second group of tabs (how mouce looks and works) begins. */
const SECOND_GROUP: HomeTabId = "customize";

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

interface LensBox {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * Follows the selected tab with the selection lens. It jumps into place the first time and
 * after layout changes (the sidebar turning into a tab bar), and glides when the tab changes.
 */
function useLens(selected: HomeTabId, tabs: Partial<Record<HomeTabId, HTMLButtonElement | null>>) {
  const navRef = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState<LensBox | null>(null);
  const [glide, setGlide] = useState(false);

  const measure = useCallback(
    (animate: boolean) => {
      const el = tabs[selected];
      if (!el) return;
      setGlide(animate);
      setBox({ x: el.offsetLeft, y: el.offsetTop, w: el.offsetWidth, h: el.offsetHeight });
    },
    [selected, tabs],
  );

  const first = useRef(true);
  const latest = useRef(measure);
  latest.current = measure;
  useLayoutEffect(() => {
    measure(!first.current);
    first.current = false;
  }, [measure]);

  // Subscribed once: re-subscribing on every tab change would fire the observer's first
  // callback straight away and cut the glide short.
  useEffect(() => {
    const nav = navRef.current;
    if (!nav || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => latest.current(false));
    observer.observe(nav);
    return () => observer.disconnect();
  }, []);

  return { navRef, box, glide };
}

export function HomeWindow() {
  const [tab, setTab] = useState<HomeTabId>(() => {
    const start = initialHomeTab();
    return isTab(start) ? start : "home";
  });
  const { settings } = useSettings();
  const now = useNow();
  const tabRefs = useRef<Partial<Record<HomeTabId, HTMLButtonElement | null>>>({});
  const { navRef, box, glide } = useLens(tab, tabRefs.current);

  useEffect(() => {
    let alive = true;
    const unlisten = onHomeTab((t) => alive && isTab(t) && setTab(t)).catch(() => null);
    return () => {
      alive = false;
      unlisten.then((un) => un?.()).catch(() => {});
    };
  }, []);

  // Arrow keys move between tabs, in either direction the list is laid out.
  function onTabKey(e: KeyboardEvent<HTMLButtonElement>) {
    const i = TABS.findIndex((t) => t.id === tab);
    let next: number | null = null;
    if (e.key === "ArrowDown" || e.key === "ArrowRight") next = (i + 1) % TABS.length;
    if (e.key === "ArrowUp" || e.key === "ArrowLeft") next = (i - 1 + TABS.length) % TABS.length;
    if (e.key === "Home") next = 0;
    if (e.key === "End") next = TABS.length - 1;
    if (next === null) return;
    e.preventDefault();
    const id = TABS[next].id;
    setTab(id);
    tabRefs.current[id]?.focus();
  }

  const label = TABS.find((t) => t.id === tab)?.label ?? "";

  return (
    <div className="home">
      <nav className="home-sidebar" aria-label="mouce">
        <div className="home-nav glass" role="tablist" aria-orientation="vertical" ref={navRef}>
          {box && (
            <span
              className={"nav-lens" + (glide ? "" : " nav-lens--still")}
              aria-hidden="true"
              style={{ width: box.w, height: box.h, transform: `translate(${box.x}px, ${box.y}px)` }}
            />
          )}
          {TABS.map((t) => (
            <FragmentWithGap key={t.id} gap={t.id === SECOND_GROUP}>
              <button
                ref={(el) => {
                  tabRefs.current[t.id] = el;
                }}
                type="button"
                role="tab"
                id={`tab-${t.id}`}
                aria-selected={tab === t.id}
                aria-controls={`panel-${t.id}`}
                tabIndex={tab === t.id ? 0 : -1}
                className="nav-item"
                onClick={() => setTab(t.id)}
                onKeyDown={onTabKey}
              >
                <Icon name={t.icon} />
                {t.label}
              </button>
            </FragmentWithGap>
          ))}
        </div>
      </nav>
      <main className="home-main" role="tabpanel" id={`panel-${tab}`} aria-labelledby={`tab-${tab}`}>
        <header className="home-header">
          <div>
            <h1 className={tab === "home" ? "large-title" : "title"}>
              {tab === "home" ? greeting(now) : label}
            </h1>
            {tab === "home" && <p className="subtitle">{longDate(now)}</p>}
          </div>
          {tab === "home" && (
            <button type="button" className="primary" onClick={() => void openChat().catch(() => {})}>
              <Icon name="sparkle" size={16} />
              Ask mouce
            </button>
          )}
        </header>
        {tab === "home" && <HomeTab settings={settings} today={now} onNavigate={setTab} />}
        {tab === "calendar" && <CalendarTab today={now} />}
        {tab === "reminders" && <RemindersTab />}
        {tab === "customize" && <Customize />}
        {tab === "settings" && <Settings />}
      </main>
    </div>
  );
}

function FragmentWithGap(props: { gap: boolean; children: ReactNode }) {
  return (
    <>
      {props.gap && <span className="nav-group-gap" aria-hidden="true" />}
      {props.children}
    </>
  );
}
