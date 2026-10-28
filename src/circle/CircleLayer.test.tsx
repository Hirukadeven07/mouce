import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/ipc");

import * as ipc from "../lib/ipc";
import { CircleLayer, HINT, TOO_SMALL } from "./CircleLayer";
import { pathData, selectionFrom } from "./selection";

const FRAME = "data:image/jpeg;base64,RlJBTUU=";

function layer() {
  return document.querySelector(".circle-layer") as HTMLElement;
}

/** Draws through `points` with the left button. */
function draw(points: Array<[number, number]>) {
  const [first, ...rest] = points;
  fireEvent.pointerDown(layer(), { clientX: first[0], clientY: first[1], button: 0 });
  rest.slice(0, -1).forEach(([x, y]) => {
    fireEvent.pointerMove(layer(), { clientX: x, clientY: y, buttons: 1 });
  });
  const last = rest[rest.length - 1] ?? first;
  fireEvent.pointerUp(layer(), { clientX: last[0], clientY: last[1], button: 0 });
}

describe("CircleLayer", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(ipc.circleFrame).mockResolvedValue(FRAME);
    vi.mocked(ipc.circleReady).mockResolvedValue();
    vi.mocked(ipc.circleFinish).mockResolvedValue();
    vi.mocked(ipc.circleCancel).mockResolvedValue();
  });

  it("shows the frozen screen and only then asks to be shown", async () => {
    render(<CircleLayer />);
    expect(screen.getByRole("status")).toHaveTextContent(HINT);
    const img = await waitFor(() => {
      const found = document.querySelector("img.circle-frame");
      expect(found).not.toBeNull();
      return found as HTMLImageElement;
    });
    expect(img).toHaveAttribute("src", FRAME);
    expect(ipc.circleReady).not.toHaveBeenCalled();
    fireEvent.load(img);
    expect(ipc.circleReady).toHaveBeenCalled();
  });

  it("sends the box around what was circled, as parts of the screen", async () => {
    render(<CircleLayer />);
    // jsdom's window is 1024 x 768.
    draw([
      [256, 192],
      [512, 200],
      [500, 384],
      [260, 380],
    ]);
    await waitFor(() => expect(ipc.circleFinish).toHaveBeenCalledTimes(1));
    const sel = vi.mocked(ipc.circleFinish).mock.calls[0][0];
    expect(sel.x).toBeCloseTo(0.25);
    expect(sel.y).toBeCloseTo(0.25);
    expect(sel.w).toBeCloseTo(0.25);
    expect(sel.h).toBeCloseTo(0.25);
  });

  it("draws the line while circling", () => {
    render(<CircleLayer />);
    fireEvent.pointerDown(layer(), { clientX: 10, clientY: 10, button: 0 });
    fireEvent.pointerMove(layer(), { clientX: 50, clientY: 12, buttons: 1 });
    expect(document.querySelector(".circle-ink path")).toHaveAttribute("d", "M10 10 L50 12");
  });

  it("a click or a tiny scribble asks for a bigger circle", async () => {
    render(<CircleLayer />);
    draw([
      [100, 100],
      [103, 102],
    ]);
    expect(await screen.findByText(TOO_SMALL)).toBeInTheDocument();
    expect(ipc.circleFinish).not.toHaveBeenCalled();
    expect(document.querySelector(".circle-ink path")).toBeNull();
  });

  it("shows why finishing failed and lets the user try again", async () => {
    vi.mocked(ipc.circleFinish).mockRejectedValueOnce("Draw around something a bit bigger");
    render(<CircleLayer />);
    draw([
      [100, 100],
      [300, 300],
    ]);
    expect(await screen.findByText("Draw around something a bit bigger")).toBeInTheDocument();
    draw([
      [100, 100],
      [400, 400],
    ]);
    await waitFor(() => expect(ipc.circleFinish).toHaveBeenCalledTimes(2));
  });

  it("Esc cancels", () => {
    render(<CircleLayer />);
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(ipc.circleCancel).toHaveBeenCalled();
  });

  it("a right click cancels", () => {
    render(<CircleLayer />);
    fireEvent.pointerDown(layer(), { clientX: 10, clientY: 10, button: 2 });
    expect(ipc.circleCancel).toHaveBeenCalled();
  });

  it("closes itself when there is no frozen screen to show", async () => {
    vi.mocked(ipc.circleFrame).mockRejectedValue("There is nothing to circle");
    render(<CircleLayer />);
    await waitFor(() => expect(ipc.circleCancel).toHaveBeenCalled());
  });
});

describe("selectionFrom", () => {
  it("is the box around the points as parts of the area", () => {
    const pts = [
      { x: 10, y: 20 },
      { x: 60, y: 5 },
      { x: 30, y: 45 },
    ];
    expect(selectionFrom(pts, 100, 50)).toEqual({ x: 0.1, y: 0.1, w: 0.5, h: 0.8 });
  });

  it("keeps points that left the area at its edge", () => {
    const pts = [
      { x: -20, y: 10 },
      { x: 50, y: 80 },
    ];
    expect(selectionFrom(pts, 100, 50)).toEqual({ x: 0, y: 0.2, w: 0.5, h: 0.8 });
  });

  it("is null for nothing, a dot or a line", () => {
    expect(selectionFrom([], 100, 100)).toBeNull();
    expect(selectionFrom([{ x: 5, y: 5 }], 100, 100)).toBeNull();
    const line = [
      { x: 0, y: 50 },
      { x: 100, y: 52 },
    ];
    expect(selectionFrom(line, 100, 100)).toBeNull();
  });
});

describe("pathData", () => {
  it("moves to the first point and draws lines through the rest", () => {
    expect(pathData([])).toBe("");
    expect(
      pathData([
        { x: 1, y: 2 },
        { x: 3, y: 4 },
        { x: 5, y: 6 },
      ]),
    ).toBe("M1 2 L3 4 L5 6");
  });
});
