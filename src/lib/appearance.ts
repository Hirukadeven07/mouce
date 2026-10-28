// Puts the Liquid Glass setting on <html data-glass="on|off"> in every page window, and adds
// the lens filter the glass uses to bend what is under its edges (see app.css).
import { getSettings, onSettingsChanged } from "./ipc";

export const LENS_FILTER_ID = "glass-lens";

export function setGlass(root: HTMLElement, on: boolean): void {
  root.dataset.glass = on ? "on" : "off";
}

// A displacement map: red rises toward the left edge and green toward the top, with a
// soft neutral middle, so only the rim of a glass surface shifts what it shows.
const LENS_MAP =
  "data:image/svg+xml," +
  encodeURIComponent(
    `<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100" preserveAspectRatio="none">` +
      `<defs>` +
      `<linearGradient id="r" x1="0" x2="1"><stop offset="0" stop-color="#f00"/><stop offset="1" stop-color="#000"/></linearGradient>` +
      `<linearGradient id="g" x1="0" x2="0" y1="0" y2="1"><stop offset="0" stop-color="#0f0"/><stop offset="1" stop-color="#000"/></linearGradient>` +
      `<filter id="b"><feGaussianBlur stdDeviation="5"/></filter>` +
      `</defs>` +
      `<rect width="100" height="100" fill="url(#r)"/>` +
      `<rect width="100" height="100" fill="url(#g)" style="mix-blend-mode:screen"/>` +
      `<rect x="9" y="9" width="82" height="82" rx="18" fill="rgb(128,128,0)" filter="url(#b)"/>` +
      `</svg>`,
  );

function addLensFilter(doc: Document): void {
  if (doc.getElementById(LENS_FILTER_ID)) return;
  const holder = doc.createElement("div");
  holder.setAttribute("aria-hidden", "true");
  holder.style.cssText = "position:absolute;width:0;height:0;overflow:hidden";
  holder.innerHTML =
    `<svg width="0" height="0" focusable="false">` +
    `<filter id="${LENS_FILTER_ID}" x="0" y="0" width="100%" height="100%" color-interpolation-filters="sRGB">` +
    `<feImage href="${LENS_MAP}" x="0" y="0" width="100%" height="100%" preserveAspectRatio="none" result="map"/>` +
    `<feDisplacementMap in="SourceGraphic" in2="map" scale="22" xChannelSelector="R" yChannelSelector="G"/>` +
    `</filter></svg>`;
  doc.body.appendChild(holder);
}

/** Follows the saved setting for the life of the window. Glass is on until it is known. */
export function followGlassSetting(doc: Document = document): void {
  const root = doc.documentElement;
  setGlass(root, true);
  addLensFilter(doc);
  let known = false;
  onSettingsChanged((s) => {
    known = true;
    setGlass(root, s.liquid_glass);
  }).catch(() => {});
  getSettings()
    .then((s) => !known && setGlass(root, s.liquid_glass))
    .catch(() => {});
}
