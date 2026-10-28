import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings } from "./types";

vi.mock("./ipc");

import { followGlassSetting, LENS_FILTER_ID } from "./appearance";
import * as ipc from "./ipc";

const settings = (liquid_glass: boolean) => ({ liquid_glass }) as Settings;

describe("followGlassSetting", () => {
  let changed: ((s: Settings) => void) | undefined;

  beforeEach(() => {
    vi.resetAllMocks();
    document.documentElement.removeAttribute("data-glass");
    document.getElementById(LENS_FILTER_ID)?.closest("div")?.remove();
    changed = undefined;
    vi.mocked(ipc.onSettingsChanged).mockImplementation(async (cb) => {
      changed = cb;
      return () => {};
    });
  });

  it("starts on, then follows the saved setting", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue(settings(false));
    followGlassSetting();
    expect(document.documentElement.dataset.glass).toBe("on");
    await vi.waitFor(() => expect(document.documentElement.dataset.glass).toBe("off"));
  });

  it("follows changes made in another window", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue(settings(true));
    followGlassSetting();
    await vi.waitFor(() => expect(changed).toBeDefined());
    changed!(settings(false));
    expect(document.documentElement.dataset.glass).toBe("off");
    changed!(settings(true));
    expect(document.documentElement.dataset.glass).toBe("on");
  });

  it("adds the lens filter once", () => {
    vi.mocked(ipc.getSettings).mockResolvedValue(settings(true));
    followGlassSetting();
    followGlassSetting();
    expect(document.querySelectorAll(`#${LENS_FILTER_ID}`)).toHaveLength(1);
  });
});
