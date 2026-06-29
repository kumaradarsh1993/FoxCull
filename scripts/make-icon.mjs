// Rasterize assets/icon.svg -> shared PNG touchpoints, then run:
//   npm run tauri icon assets/icon-1024.png
// which regenerates the full src-tauri/icons/* set (png/ico/icns/Store logos).
import { Resvg } from "@resvg/resvg-js";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";

const svg = readFileSync(new URL("../assets/icon.svg", import.meta.url));

function render(width, target) {
  const resvg = new Resvg(svg, { fitTo: { mode: "width", value: width } });
  const url = new URL(target, import.meta.url);
  mkdirSync(new URL(".", url), { recursive: true });
  writeFileSync(url, resvg.render().asPng());
  console.log(`wrote ${target.replace("../", "")} (${width}x${width})`);
}

render(1024, "../assets/icon-1024.png");
render(512, "../docs/images/foxcull-codex-icon.png");
render(256, "../static/favicon.png");
