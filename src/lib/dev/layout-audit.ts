// @ts-nocheck — dev-only DOM probe; typing every element access adds nothing.
// DEV-ONLY layout audit. Installed with the fake backend (see hooks.client.ts).
//
// In the browser console: `__audit()` scans the main window, `__audit(".pop")`
// scans one floating layer (a popover, menu or dialog). It reports:
//   OVERLAP       two pieces of text/controls whose boxes intersect
//   CLIPPED-TEXT  text cut off by overflow:hidden without an ellipsis
//   OFFSCREEN     something past the window edge that nothing can scroll to
//   PAGE-HSCROLL  the app shell is wider than the window
// Written for the 2026-09 audit, after the owner's Mac showed overlaps the
// Windows-only QA had never seen (Segoe UI is absent there; the fallback font
// is wider). Run it at every window size and UI scale in the responsive
// contract, in each theme, with every popover open.

export function installLayoutAudit() {
  (window as unknown as { __audit: (rootSel?: string) => string[] }).__audit = (rootSel?: string): string[] => {
    const root = rootSel ? document.querySelector(rootSel) : document.body;
    if (!root) return ["no root " + rootSel];
    const vw = innerWidth, vh = innerHeight;
    const visible = (el) => {
      const cs = getComputedStyle(el);
      if (cs.visibility === "hidden" || cs.display === "none" || +cs.opacity === 0) return false;
      const r = el.getBoundingClientRect();
      return r.width > 1 && r.height > 1;
    };
    // Skip anything inside a floating layer unless it IS the root we're scanning.
    const floating = (el) => {
      for (let p = el; p && p !== root; p = p.parentElement) {
        const cs = getComputedStyle(p);
        if (cs.position === "fixed") return true;
      }
      return false;
    };
    const ownText = (el) => [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim().length > 0);
    const leaves = [...root.querySelectorAll("*")].filter((el) => {
      if (!visible(el) || (!rootSel && floating(el))) return false;
      const tag = el.tagName;
      if (["BUTTON", "INPUT", "SELECT", "TEXTAREA", "KBD"].includes(tag)) return true;
      if (tag === "svg" || el.closest("svg")) return false;
      // A small button is one unit; a big one (a grid/filmstrip tile) holds
      // badges and marks that can collide with each other, so look inside it.
      const btn = el.closest("button");
      if (btn && btn !== el) {
        const br = btn.getBoundingClientRect();
        if (br.width < 80 || br.height < 60) return false;
      }
      return ownText(el);
    });
    const desc = (el) => {
      const t = (el.innerText || el.value || el.getAttribute("aria-label") || el.title || "").trim().replace(/\s+/g, " ").slice(0, 40);
      const cls = typeof el.className === "string" ? el.className.split(" ").filter((c) => !c.startsWith("s-")).slice(0, 2).join(".") : "";
      return `${el.tagName.toLowerCase()}${cls ? "." + cls : ""}"${t}"`;
    };
    // Clip to what is actually visible inside scroll containers.
    const clipRect = (el) => {
      let r = el.getBoundingClientRect();
      let x1 = r.left, y1 = r.top, x2 = r.right, y2 = r.bottom;
      for (let p = el.parentElement; p; p = p.parentElement) {
        const cs = getComputedStyle(p);
        if (/(auto|scroll|hidden|clip)/.test(cs.overflow + cs.overflowX + cs.overflowY)) {
          const pr = p.getBoundingClientRect();
          x1 = Math.max(x1, pr.left); y1 = Math.max(y1, pr.top); x2 = Math.min(x2, pr.right); y2 = Math.min(y2, pr.bottom);
        }
      }
      return { x1, y1, x2, y2 };
    };
    const out = [];
    const rects = leaves.map((el) => ({ el, r: clipRect(el) })).filter((o) => o.r.x2 - o.r.x1 > 1 && o.r.y2 - o.r.y1 > 1);
    for (let i = 0; i < rects.length; i++) {
      for (let j = i + 1; j < rects.length; j++) {
        const a = rects[i], b = rects[j];
        if (a.el.contains(b.el) || b.el.contains(a.el)) continue;
        const ix = Math.min(a.r.x2, b.r.x2) - Math.max(a.r.x1, b.r.x1);
        const iy = Math.min(a.r.y2, b.r.y2) - Math.max(a.r.y1, b.r.y1);
        if (ix > 2 && iy > 2) {
          // Text that wraps has one box per line; its bounding box spans lines
          // it doesn't occupy. Only a collision between actual line boxes counts.
          const lines = (el) => [...el.getClientRects()];
          const hit = lines(a.el).some((ra) =>
            lines(b.el).some(
              (rb) =>
                Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left) > 2 &&
                Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top) > 2,
            ),
          );
          if (hit) out.push(`OVERLAP ${Math.round(ix)}x${Math.round(iy)}: ${desc(a.el)} ⟂ ${desc(b.el)}`);
        }
      }
    }
    for (const { el } of rects) {
      const cs = getComputedStyle(el);
      const clips = /(hidden|clip)/.test(cs.overflowX) || /(hidden|clip)/.test(cs.overflow);
      if (el.scrollWidth > el.clientWidth + 2 && cs.textOverflow !== "ellipsis" && clips && ownText(el))
        out.push(`CLIPPED-TEXT ${el.scrollWidth}>${el.clientWidth}: ${desc(el)}`);
      const r = el.getBoundingClientRect();
      if (r.right > vw + 1 || r.bottom > vh + 1 || r.left < -1) {
        // Only flag if it isn't inside a scroll container that can reveal it.
        let scrollable = false;
        for (let p = el.parentElement; p; p = p.parentElement) {
          const pcs = getComputedStyle(p);
          if (/(auto|scroll)/.test(pcs.overflowY + pcs.overflowX) && (p.scrollHeight > p.clientHeight || p.scrollWidth > p.clientWidth)) { scrollable = true; break; }
        }
        if (!scrollable) out.push(`OFFSCREEN (${Math.round(r.left)},${Math.round(r.top)})-(${Math.round(r.right)},${Math.round(r.bottom)}) vp ${vw}x${vh}: ${desc(el)}`);
      }
    }
    // The Settings-popover bug class: a height-capped flex column shrinks its
    // children (flex-shrink:1), and a child with an explicit min-height gives up
    // its content-sized minimum, so its content spills onto the next row.
    for (const el of root.querySelectorAll("*")) {
      const cs = getComputedStyle(el);
      if (!cs.display.includes("flex") || !cs.flexDirection.startsWith("column")) continue;
      for (const kid of el.children) {
        if (!visible(kid)) continue;
        const kcs = getComputedStyle(kid);
        if (kcs.position === "absolute" || kcs.position === "fixed") continue;
        // Resize rails deliberately let their toggle overhang a thin bar.
        if (kid.getAttribute("role") === "separator") continue;
        if (kid.scrollHeight > kid.clientHeight + 2 && !/(auto|scroll)/.test(kcs.overflowY))
          out.push(`SQUASHED ${kid.clientHeight}<${kid.scrollHeight}: ${desc(kid)} in ${desc(el)}`);
      }
    }
    // Horizontal page scroll is always a bug in an app shell.
    if (document.documentElement.scrollWidth > vw + 1) out.push(`PAGE-HSCROLL ${document.documentElement.scrollWidth}>${vw}`);
    return [...new Set(out)];
  };

  // Magnify a region for screenshots WITHOUT reflowing: a transform on <html>
  // leaves layout untouched. `__zoom(x, y, 2)` centres on viewport point (x, y).
  const w = window as unknown as Record<string, unknown>;
  w.__zoom = (x: number, y: number, s = 2) => {
    const el = document.documentElement;
    el.style.transformOrigin = `${x}px ${y}px`;
    el.style.transform = `scale(${s})`;
  };
  w.__unzoom = () => {
    document.documentElement.style.transform = "";
  };

  // Walk every surface FoxCull has and audit each one. Needs a folder open
  // (the fake backend restores the last one). Returns { surface: findings[] };
  // an empty array is a pass. Run it per window size / UI scale / theme.
  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
  const btn = (t: string) =>
    [...document.querySelectorAll("button")].find((b) =>
      [b.getAttribute("aria-label"), b.title, b.textContent?.trim()].some(
        (x) => x && x.toLowerCase().startsWith(t.toLowerCase()),
      ),
    ) as HTMLButtonElement | undefined;
  const key = (k: string) => window.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
  const audit = (w.__audit as (s?: string) => string[]);
  w.__sweep = async () => {
    const res: Record<string, string[]> = {};
    const probe = async (name: string, open: () => unknown, sel: string, close: () => unknown) => {
      try {
        await open();
        await sleep(450);
        res[name] = document.querySelector(sel) ? audit(sel) : [`NOT OPENED ${sel}`];
      } catch (e) {
        res[name] = [`ERR ${e}`];
      }
      try {
        await close();
      } catch {
        /* already closed */
      }
      await sleep(250);
    };
    const click = (t: string) => () => btn(t)!.click();
    const viaSettings = (t: string) => async () => {
      btn("Settings")!.click();
      await sleep(250);
      btn(t)!.click();
    };
    btn("Grid")?.click();
    await sleep(600);
    res.grid = audit();
    btn("Details")?.click();
    await sleep(600);
    res.details = audit();
    btn("Focus")?.click();
    await sleep(900);
    res.focus = audit();
    btn("Grid")?.click();
    await sleep(600);
    await probe("menu:arrange", click("Sort, group"), ".arrangeMenu", click("Sort, group"));
    await probe("menu:filters", click("Filters"), ".filtermenu", click("Filters"));
    const caret = () => (document.querySelector(".prep")?.nextElementSibling as HTMLElement).click();
    await probe("menu:prepare", caret, ".prepMenu", caret);
    await probe("menu:clear", click("Clear ratings"), ".clearMenu", click("Clear ratings"));
    await probe("menu:cast", () => (document.querySelector(".castBtn") as HTMLElement).click(), ".castMenu", () =>
      (document.querySelector(".castBtn") as HTMLElement).click(),
    );
    await probe("menu:settings", click("Settings"), ".pop", click("Settings"));
    const cell = document.querySelector(".cell");
    await probe(
      "menu:item-context",
      () => cell!.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: innerWidth - 40, clientY: innerHeight - 40 })),
      ".cm",
      () => key("Escape"),
    );
    const folder = document.querySelector(".tree-body button");
    await probe(
      "menu:folder-context",
      () => folder!.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 60, clientY: innerHeight - 40 })),
      ".cm",
      () => key("Escape"),
    );
    await probe("dialog:shortcuts", () => key("?"), ".kbGuide", () => key("Escape"));
    await probe("dialog:about", viaSettings("What you're running"), ".aboutBox", () => key("Escape"));
    await probe("dialog:controller", viaSettings("Pair a PS5"), ".panel", () => key("Escape"));
    await probe("dialog:excludes", viaSettings("Folders FoxCull never scans"), ".panel", () => key("Escape"));
    btn("Edit")?.click();
    await sleep(1500);
    res.edit = audit();
    ([...document.querySelectorAll("button")].find((b) => b.textContent?.trim() === "Library") as HTMLButtonElement | undefined)?.click();
    await sleep(600);
    return Object.fromEntries(Object.entries(res).filter(([, v]) => v.length));
  };
}
