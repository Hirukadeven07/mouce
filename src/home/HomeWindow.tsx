import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import logoUrl from "../assets/mouce.png";
import { ChatView } from "../chat/ChatWindow";
import { closeChatPanel, initialHomeTab, onHomeTab, openChat } from "../lib/ipc";
import type { HomeTabId } from "../lib/types";
import { Settings } from "../settings/Settings";
import { useSettings } from "../settings/useSettings";
import { Icon, type IconName } from "../ui/Icon";
import { CalendarTab } from "./CalendarTab";
import { Customize } from "./Customize";
import { greeting, longDate } from "./dates";
import { HomeTab } from "./HomeTab";
import { RemindersTab } from "./RemindersTab";

const TABS: Array<{ id: HomeTabId; label: string; icon: IconName }> = [
  { id: "home", label: "Home", icon: "home" },
  { id: "chat", label: "Chat", icon: "chat" },
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
  // Keyboard users arrowing through the tabs keep their place; the chat takes focus only when
  // it is chosen by a click or moved here from the panel.
  const [arrowed, setArrowed] = useState(false);

  useEffect(() => {
    let alive = true;
    const unlisten = onHomeTab((t) => {
      if (!alive || !isTab(t)) return;
      setArrowed(false);
      setTab(t);
    }).catch(() => null);
    return () => {
      alive = false;
      unlisten.then((un) => un?.()).catch(() => {});
    };
  }, []);

  // The chat shows here full size, so the small panel beside the mouce closes.
  useEffect(() => {
    if (tab === "chat") void closeChatPanel().catch(() => {});
  }, [tab]);

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
    setArrowed(true);
    setTab(id);
    tabRefs.current[id]?.focus();
  }

  const label = TABS.find((t) => t.id === tab)?.label ?? "";

  return (
    <div className={"home" + (tab === "chat" ? " home--chat" : "")} data-tab={tab}>
      <div className="aurora" aria-hidden="true" />
      <nav className="home-sidebar" aria-label="mouce">
        <div className="home-rail">
          <div className="home-brand" aria-hidden="true">
            <img src={logoUrl} alt="" width={26} height={26} />
            <span>mouce</span>
          </div>
          <div className="home-nav" role="tablist" aria-orientation="vertical" ref={navRef}>
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
                  onClick={() => {
                    setArrowed(false);
                    setTab(t.id);
                  }}
                  onKeyDown={onTabKey}
                >
                  <Icon name={t.icon} />
                  {t.label}
                </button>
              </FragmentWithGap>
            ))}
          </div>
          {settings?.chat_hotkey && (
            <p className="home-hint">
              <span>Chat from any app</span>
              <span className="home-keys">
                {settings.chat_hotkey.split("+").map((key) => (
                  <kbd key={key}>{key}</kbd>
                ))}
              </span>
            </p>
          )}
        </div>
      </nav>
      <main className="home-main" role="tabpanel" id={`panel-${tab}`} aria-labelledby={`tab-${tab}`}>
        {tab !== "chat" && (
          <header className="home-header" key={tab}>
            <div className="home-heading">
              <h1 className={tab === "home" ? "large-title" : "title"}>
                {tab === "home" ? <Greeting text={greeting(now)} /> : label}
              </h1>
              {tab === "home" && <p className="subtitle">{longDate(now)}</p>}
            </div>
            {tab === "home" && (
              <button type="button" className="primary ask-button" onClick={() => void openChat().catch(() => {})}>
                <Icon name="sparkle" size={16} />
                Ask mouce
              </button>
            )}
          </header>
        )}
        {tab === "home" && <HomeTab settings={settings} today={now} onNavigate={setTab} />}
        {tab === "chat" && (
          <ChatView
            mode="full"
            autoFocus={!arrowed}
            onPopOut={() => {
              setTab("home");
              void openChat().catch(() => {});
            }}
          />
        )}
        {tab === "calendar" && <CalendarTab today={now} />}
        {tab === "reminders" && <RemindersTab today={now} />}
        {tab === "customize" && <Customize />}
        {tab === "settings" && <Settings />}
      </main>
    </div>
  );
}

/** "Good *evening.*": the greeting's last word set in italics, as a headline would be. */
function Greeting(props: { text: string }) {
  const cut = props.text.lastIndexOf(" ");
  return (
    <>
      {props.text.slice(0, cut + 1)}
      <em>{props.text.slice(cut + 1)}.</em>
    </>
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
