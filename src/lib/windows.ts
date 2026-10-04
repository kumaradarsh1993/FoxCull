// Which window this is. Every FoxCull window loads the same app; the label
// decides what it shows: "main" (the library), "edit" or "merge". In the
// browser harness, `?window=edit` stands in for the label.

export type WindowKind = "main" | "edit" | "merge";

export function windowKind(): WindowKind {
  try {
    const q = new URLSearchParams(location.search).get("window");
    if (q === "edit" || q === "merge") return q;
    const meta = (window as unknown as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } } })
      .__TAURI_INTERNALS__?.metadata?.currentWindow?.label;
    if (meta === "edit" || meta === "merge") return meta;
  } catch {
    /* not in Tauri */
  }
  return "main";
}

/** Close this window (Edit/Merge). The work it started keeps running. */
export async function closeThisWindow() {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().close();
  } catch {
    history.back();
  }
}
