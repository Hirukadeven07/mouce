import { fireEvent } from "@testing-library/react";
import { expect, it, vi } from "vitest";

vi.mock("./lib/ipc");

import * as ipc from "./lib/ipc";

it("every window blocks the default right-click menu", async () => {
  vi.mocked(ipc.windowLabel).mockReturnValue("other");
  document.body.innerHTML = '<div id="root"></div>';
  await import("./main");
  expect(fireEvent.contextMenu(document.body)).toBe(false);
});
