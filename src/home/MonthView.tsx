import { useEffect, useRef, type KeyboardEvent } from "react";
import { addDays, isoDate, monthGrid, WEEKDAYS } from "./dates";

/** How far each arrow key moves the selected day. */
const ARROW_DAYS: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 };

/**
 * A Monday-first month grid with today highlighted, and a dot on each day in `marked`.
 * With `onSelect`, days can be picked by click or with the arrow keys; the selected day is
 * the one that takes Tab.
 */
export function MonthView(props: {
  year: number;
  month: number;
  today: Date;
  large?: boolean;
  /** Days ("YYYY-MM-DD") that have something on them. */
  marked?: ReadonlySet<string>;
  selected?: string;
  onSelect?: (date: string) => void;
}) {
  const { year, month, today, large, marked, selected, onSelect } = props;
  const cells = monthGrid(year, month, today);
  const gridRef = useRef<HTMLDivElement>(null);
  // Set when the arrow keys move the selection, so focus follows it into the new cell.
  const moved = useRef(false);

  useEffect(() => {
    if (!moved.current) return;
    moved.current = false;
    gridRef.current?.querySelector<HTMLButtonElement>('[aria-selected="true"] button')?.focus();
  }, [selected, year, month]);

  function onKeyDown(e: KeyboardEvent, date: string) {
    const days = ARROW_DAYS[e.key];
    if (days === undefined || !onSelect) return;
    e.preventDefault();
    moved.current = true;
    onSelect(addDays(date, days));
  }

  return (
    <div className={"month" + (large ? " month--large" : "")} role="grid" aria-readonly="true" ref={gridRef}>
      <div className="month-row" role="row">
        {WEEKDAYS.map((d, i) => (
          <span key={i} className="month-head" role="columnheader">
            {d}
          </span>
        ))}
      </div>
      {Array.from({ length: cells.length / 7 }, (_, w) => (
        <div className="month-row" role="row" key={w}>
          {cells.slice(w * 7, w * 7 + 7).map((c) => {
            const date = isoDate(c.date);
            const isSelected = onSelect !== undefined && date === selected;
            const className =
              "month-day" +
              (c.inMonth ? "" : " month-day--other") +
              (c.isToday ? " month-day--today" : "") +
              (isSelected ? " month-day--selected" : "") +
              (marked?.has(date) ? " month-day--marked" : "");
            const label = c.date.toLocaleDateString("en-GB", { weekday: "long", day: "numeric", month: "long" });
            if (!onSelect) {
              return (
                <span
                  key={date}
                  role="gridcell"
                  className={className}
                  aria-current={c.isToday ? "date" : undefined}
                >
                  {c.date.getDate()}
                </span>
              );
            }
            return (
              <span key={date} role="gridcell" aria-selected={isSelected} className="month-cell">
                <button
                  type="button"
                  className={className}
                  aria-label={label + (marked?.has(date) ? ", has plans" : "")}
                  aria-current={c.isToday ? "date" : undefined}
                  tabIndex={isSelected ? 0 : -1}
                  onClick={() => onSelect(date)}
                  onKeyDown={(e) => onKeyDown(e, date)}
                >
                  {c.date.getDate()}
                </button>
              </span>
            );
          })}
        </div>
      ))}
    </div>
  );
}
