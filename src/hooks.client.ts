// Runs once in the webview before the app mounts.
//
// In a plain browser during `npm run dev` there is no Tauri bridge, so install
// the dev-only fake backend (see src/lib/dev/mock-ipc.ts) and the UI can be
// inspected with a realistic library. Inside the app `__TAURI_INTERNALS__`
// already exists, and production builds drop the whole branch.
export async function init() {
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
    const [{ installMockIpc }, { installLayoutAudit }] = await Promise.all([
      import("$lib/dev/mock-ipc"),
      import("$lib/dev/layout-audit"),
    ]);
    installMockIpc();
    installLayoutAudit();
  }
}
