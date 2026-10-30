import type { CalendarKind } from "../lib/types";

// Messages that ask mouce to remember something rather than answer a question. Only the
// start of the message is read, so "how do I remind myself…" stays a question.
const REMINDER = /^\s*(please\s+|can you\s+|could you\s+)?(remind me\b|set (a |an )?(reminder|alarm)\b|add (a )?reminder\b|new reminder\b)/i;
const EVENT_TO_CALENDAR =
  /^\s*(please\s+|can you\s+|could you\s+)?(add|put|schedule|book)\b.*\b(to|in|on|into) (my |the )?(calendar|diary|schedule)\b/i;
const NEW_EVENT = /^\s*(please\s+)?(add|create|new|schedule) (an? )?(event|appointment|meeting)\b/i;

/** Whether `text` asks mouce to add a reminder or an event, and which. */
export function calendarIntent(text: string): CalendarKind | null {
  if (REMINDER.test(text)) return "reminder";
  if (EVENT_TO_CALENDAR.test(text) || NEW_EVENT.test(text)) return "event";
  return null;
}

// Questions about the user's own plans ("what's on tomorrow?", "am I free on Friday?").
// They are answered from the calendar, so it goes to the AI with them.
const PLANS = [
  /\b(am i|are we) (free|busy|available)\b/i,
  /\bdo i have (anything|any|plans|something|time)\b/i,
  /\bwhen('?s| is| are) my\b/i,
  /\bmy (calendar|schedule|agenda|plans|reminders|day|week|weekend)\b/i,
  /\bwhat('?s| is| have i got| do i have) (on|planned|happening|coming up|next)\b(?!.*\bscreen\b)/i,
];

/** Whether `text` asks about the user's plans. */
export function plansQuestion(text: string): boolean {
  return PLANS.some((re) => re.test(text));
}
