import { describe, expect, it } from "vitest";
import { calendarIntent, plansQuestion } from "./calendarIntent";

describe("calendarIntent", () => {
  it("spots a reminder", () => {
    for (const text of [
      "remind me to call mum at 6",
      "Remind me tomorrow about the bins",
      "please remind me to stretch every hour",
      "can you remind me to pay rent on the 1st",
      "set a reminder for the dentist",
      "Set an alarm for 7am",
      "add reminder: water the plants",
    ]) {
      expect(calendarIntent(text), text).toBe("reminder");
    }
  });

  it("spots an event for the calendar", () => {
    for (const text of [
      "add dentist tomorrow at 3pm to my calendar",
      "put the team lunch on Friday in my calendar",
      "Schedule gym every Thursday at 7 in my diary",
      "add an event: Sam's birthday on 4 Dec",
      "create a meeting with Alex on Monday",
      "new appointment at the bank next week",
    ]) {
      expect(calendarIntent(text), text).toBe("event");
    }
  });

  it("leaves questions alone", () => {
    for (const text of [
      "how do I add a reminder in Outlook?",
      "what does remind me mean",
      "can you explain my calendar app",
      "add these numbers: 3 + 4",
      "What's on my screen?",
      "",
    ]) {
      expect(calendarIntent(text), text).toBeNull();
    }
  });
});

describe("plansQuestion", () => {
  it("spots questions about the user's plans", () => {
    for (const text of [
      "what's on tomorrow?",
      "What do I have on Friday",
      "am I free on Saturday afternoon?",
      "Do I have anything this weekend",
      "when is my dentist appointment?",
      "what does my week look like",
      "show me my calendar",
      "what's coming up?",
    ]) {
      expect(plansQuestion(text), text).toBe(true);
    }
  });

  it("leaves other questions alone", () => {
    for (const text of ["What's on my screen?", "what is a calendar", "how do loops work", "hi"]) {
      expect(plansQuestion(text), text).toBe(false);
    }
  });
});
