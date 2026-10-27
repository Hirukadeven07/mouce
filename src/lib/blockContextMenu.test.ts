import { fireEvent } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { blockContextMenu } from "./blockContextMenu";

describe("blockContextMenu", () => {
  let stop: (() => void) | undefined;
  afterEach(() => stop?.());

  it("cancels the right-click menu anywhere in the document", () => {
    stop = blockContextMenu(document);
    const input = document.createElement("input");
    document.body.appendChild(input);
    // fireEvent returns false when the event was default-prevented.
    expect(fireEvent.contextMenu(input)).toBe(false);
    expect(fireEvent.contextMenu(document.body)).toBe(false);
    input.remove();
  });

  it("can be removed again", () => {
    blockContextMenu(document)();
    expect(fireEvent.contextMenu(document.body)).toBe(true);
  });
});
