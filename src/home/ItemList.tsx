import { useState } from "react";
import { calendarDelete, calendarSetDone } from "../lib/ipc";
import type { CalendarItem } from "../lib/types";
import { Icon } from "../ui/Icon";
import { dayLabel, localMs } from "./dates";
import { repeatText } from "./ItemForm";

/** "Reminds 10 minutes before", for an event with a reminder. */
export function reminderText(item: CalendarItem): string | null {
  const before = item.remind_before;
  if (item.kind !== "event" || before === null) return null;
  if (item.time === null) return before === 0 ? "Reminds at 9:00 on the day" : "Reminds at 9:00 the day before";
  if (before === 0) return "Reminds at the start";
  if (before % 1440 === 0) return before === 1440 ? "Reminds a day before" : `Reminds ${before / 1440} days before`;
  if (before % 60 === 0) return before === 60 ? "Reminds an hour before" : `Reminds ${before / 60} hours before`;
  return `Reminds ${before} minutes before`;
}

/** A reminder still to do whose time has passed. */
export function isOverdue(item: CalendarItem, now: Date): boolean {
  return item.kind === "reminder" && !item.done && item.time !== null && localMs(item.date, item.time) < now.getTime();
}

/** When it is: "All day", "15:00", or with the day ("Tomorrow 15:00") when `withDay` is set. */
export function whenLabel(item: CalendarItem, now: Date, withDay: boolean): string {
  const time = item.time ?? "All day";
  if (!withDay) return time;
  return `${dayLabel(item.date, now)}${item.time ? " " + item.time : ", all day"}`;
}

/**
 * One event or reminder. A reminder has a box to tick when it is done. With `onEdit` the
 * row can be changed and deleted (deleting asks once more, in the row).
 */
function ItemRow(props: {
  item: CalendarItem;
  now: Date;
  withDay: boolean;
  onEdit?: (item: CalendarItem) => void;
}) {
  const { item, now, withDay, onEdit } = props;
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const overdue = isOverdue(item, now);
  const note = reminderText(item);
  const repeats = repeatText(item.repeat);
  const fail = (e: unknown) => setError(String(e));

  return (
    <li className={"item" + (item.done ? " item--done" : "") + (overdue ? " item--overdue" : "")}>
      {item.kind === "reminder" ? (
        <input
          type="checkbox"
          className="item-done"
          checked={item.done}
          aria-label={`Done: ${item.title}`}
          onChange={(e) => void calendarSetDone(item.id, e.target.checked).catch(fail)}
        />
      ) : (
        <span className="item-dot" aria-hidden="true" />
      )}
      <div className="item-text">
        <p className="item-title">{item.title}</p>
        <p className="item-when">
          {overdue && <span className="item-late">Overdue · </span>}
          {whenLabel(item, now, withDay || overdue)}
          {repeats && <span> · {repeats}</span>}
          {note && <span> · {note}</span>}
        </p>
        {error && (
          <p className="item-error" role="alert">
            {error}
          </p>
        )}
      </div>
      {onEdit &&
        (confirming ? (
          <div className="item-actions">
            <button type="button" onClick={() => setConfirming(false)}>
              Keep
            </button>
            <button type="button" className="danger" onClick={() => void calendarDelete(item.id).catch(fail)}>
              {item.repeat ? "Delete every time" : "Delete"}
            </button>
          </div>
        ) : (
          <div className="item-actions">
            <button
              type="button"
              className="icon-button"
              aria-label={`Change “${item.title}”`}
              title="Change"
              onClick={() => onEdit(item)}
            >
              <Icon name="compose" size={15} />
            </button>
            <button
              type="button"
              className="icon-button"
              aria-label={`Delete “${item.title}”`}
              title="Delete"
              onClick={() => setConfirming(true)}
            >
              <Icon name="close" size={13} />
            </button>
          </div>
        ))}
    </li>
  );
}

/** A list of events and reminders, or `empty` when there are none. */
export function ItemList(props: {
  items: CalendarItem[] | null;
  now: Date;
  empty: string;
  /** Say the day as well as the time (for lists that span days). */
  withDay?: boolean;
  onEdit?: (item: CalendarItem) => void;
  label?: string;
}) {
  const { items, now, empty, withDay = false, onEdit, label } = props;
  if (items === null) return <p className="empty">Loading…</p>;
  if (items.length === 0) return <p className="empty">{empty}</p>;
  return (
    <ul className="items" aria-label={label}>
      {items.map((item) => (
        <ItemRow key={`${item.id}/${item.date}`} item={item} now={now} withDay={withDay} onEdit={onEdit} />
      ))}
    </ul>
  );
}
