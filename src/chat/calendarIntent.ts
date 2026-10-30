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
