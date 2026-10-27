// WebView2's default right-click menu offers Save as, Print and similar. No Star window
// wants it, so every window cancels it. Returns a function that removes the guard.
export function blockContextMenu(doc: Document): () => void {
  const cancel = (e: Event) => e.preventDefault();
  doc.addEventListener("contextmenu", cancel);
  return () => doc.removeEventListener("contextmenu", cancel);
}
