// What is being dragged out of the grid right now. A drop target can't read
// the drag's payload until the drop itself (browsers hide it during dragover),
// so the grid publishes the count here and folder rows can say "Move 24".

export const mediaDrag = $state({ count: 0 });

const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

/** The platform's copy modifier during a drag: Option on a Mac (as in
 *  Finder), Ctrl on Windows and Linux (as in Explorer). */
export function wantsCopy(e: DragEvent): boolean {
  return isMac ? e.altKey : e.ctrlKey;
}
