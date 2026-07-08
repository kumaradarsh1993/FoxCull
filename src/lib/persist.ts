import { Store } from "@tauri-apps/plugin-store";

let store: Store | null = null;
async function s(): Promise<Store> {
  if (!store) {
    store = await Store.load("foxcull.json");
    const root = await store.get<string>("root");
    if (!root) {
      const legacy = await Store.load("foxcull-codex.json");
      const legacyRoot = await legacy.get<string>("root");
      if (legacyRoot) {
        await store.set("root", legacyRoot);
        await store.save();
      }
    }
  }
  return store;
}

export async function getLastRoot(): Promise<string | null> {
  return (await (await s()).get<string>("root")) ?? null;
}

export async function setLastRoot(root: string): Promise<void> {
  const st = await s();
  await st.set("root", root);
  await st.save();
}
