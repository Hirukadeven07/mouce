// One small line-icon set, drawn on a 20px grid with a 1.6px stroke so every icon matches
// the weight of the text beside it. Icons are decoration: the control carries the name.

const PATHS = {
  home: "M3.5 9.2 10 3.8l6.5 5.4M5.5 7.8v8.4h3.4v-4.6h2.2v4.6h3.4V7.8",
  calendar: "M4 5.5h12a.5.5 0 0 1 .5.5v10a.5.5 0 0 1-.5.5H4a.5.5 0 0 1-.5-.5V6a.5.5 0 0 1 .5-.5ZM3.5 9h13M7 3.5v3.5M13 3.5v3.5",
  reminders: "M4.5 6.2 6 7.6l2.5-2.7M4.5 12.7l1.5 1.4 2.5-2.7M11 6.5h4.5M11 13h4.5",
  customize: "M4 6h7M15 6h1M4 14h1M9 14h7M13 4.2v3.6M7 12.2v3.6",
  settings:
    "M10 7.4a2.6 2.6 0 1 0 0 5.2 2.6 2.6 0 0 0 0-5.2ZM8.6 3.5h2.8l.4 1.9 1.6.9 1.8-.6 1.4 2.4-1.4 1.3v1.2l1.4 1.3-1.4 2.4-1.8-.6-1.6.9-.4 1.9H8.6l-.4-1.9-1.6-.9-1.8.6-1.4-2.4 1.4-1.3V9.4L3.4 8.1l1.4-2.4 1.8.6 1.6-.9.4-1.9Z",
  sparkle: "M10 3.5c.5 3.4 1.9 5.2 5.5 6.5-3.6 1.3-5 3.1-5.5 6.5-.5-3.4-1.9-5.2-5.5-6.5 3.6-1.3 5-3.1 5.5-6.5Z",
  look: "M3.5 7V5a1.5 1.5 0 0 1 1.5-1.5h2M13 3.5h2A1.5 1.5 0 0 1 16.5 5v2M16.5 13v2a1.5 1.5 0 0 1-1.5 1.5h-2M7 16.5H5A1.5 1.5 0 0 1 3.5 15v-2M10 8a2 2 0 1 0 0 4 2 2 0 0 0 0-4Z",
  circle:
    "M14.6 13.8c1.2-1 1.9-2.3 1.9-3.8 0-3.3-2.9-6-6.5-6S3.5 6.7 3.5 10s2.9 6 6.5 6c.8 0 1.6-.1 2.3-.4M14.6 13.8l.2 2.7M14.6 13.8l-2.5.6",
  close: "m5.5 5.5 9 9M14.5 5.5l-9 9",
  send: "M10 15.5v-11M5.5 9 10 4.5 14.5 9",
  compose: "M9 4.5H5.5a1 1 0 0 0-1 1v9a1 1 0 0 0 1 1h9a1 1 0 0 0 1-1V11M8 12l.5-2.5 6-6 2 2-6 6L8 12Z",
  chevronLeft: "M12 4.5 6.5 10l5.5 5.5",
  chevronRight: "m8 4.5 5.5 5.5L8 15.5",
  chat: "M5 4.5h10A1.5 1.5 0 0 1 16.5 6v6a1.5 1.5 0 0 1-1.5 1.5H9.2l-3.7 2.8v-2.8H5A1.5 1.5 0 0 1 3.5 12V6A1.5 1.5 0 0 1 5 4.5Z",
  expand: "M11.5 3.5h5v5M16.5 3.5 11.2 8.8M8.5 16.5h-5v-5M3.5 16.5l5.3-5.3",
  shrink: "M16.5 3.5 11.5 8.5M11.5 4.5v4h4M3.5 16.5l5-5M8.5 15.5v-4h-4",
} as const;

export type IconName = keyof typeof PATHS | "stop";

export function Icon(props: { name: IconName; size?: number; className?: string }) {
  const { name, size = 18, className } = props;
  return (
    <svg
      className={"icon" + (className ? " " + className : "")}
      width={size}
      height={size}
      viewBox="0 0 20 20"
      aria-hidden="true"
      focusable="false"
    >
      {name === "stop" ? (
        <rect x="6" y="6" width="8" height="8" rx="1.5" fill="currentColor" />
      ) : (
        <path
          d={PATHS[name]}
          fill="none"
          stroke="currentColor"
          strokeWidth="1.6"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      )}
    </svg>
  );
}
