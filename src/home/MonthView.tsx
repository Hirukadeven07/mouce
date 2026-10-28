import { monthGrid, WEEKDAYS } from "./dates";

/** A Monday-first month grid with today highlighted. */
export function MonthView(props: { year: number; month: number; today: Date; large?: boolean }) {
  const { year, month, today, large } = props;
  const cells = monthGrid(year, month, today);
  return (
    <div className={"month" + (large ? " month--large" : "")} role="grid" aria-readonly="true">
      <div className="month-row" role="row">
        {WEEKDAYS.map((d, i) => (
          <span key={i} className="month-head" role="columnheader">
            {d}
          </span>
        ))}
      </div>
      {Array.from({ length: cells.length / 7 }, (_, w) => (
        <div className="month-row" role="row" key={w}>
          {cells.slice(w * 7, w * 7 + 7).map((c) => (
            <span
              key={c.date.toISOString()}
              role="gridcell"
              className={
                "month-day" + (c.inMonth ? "" : " month-day--other") + (c.isToday ? " month-day--today" : "")
              }
              aria-current={c.isToday ? "date" : undefined}
            >
              {c.date.getDate()}
            </span>
          ))}
        </div>
      ))}
    </div>
  );
}
