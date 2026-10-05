// Regenerates every app icon from the SVG sources in assets/icon (run: `pnpm icons`).
//
// 1. `tauri icon assets/icon/icons.json` writes all sizes to src-tauri/icons:
//    PNGs, icon.icns, and the Android set (adaptive foreground/background/
//    monochrome, legacy and round icons). It scales one source per output.
// 2. src-tauri/icons/icon.ico is then rebuilt with hand-tuned layers: the shop
//    PC runs at 100 % scaling, where Windows draws the title bar and tray at
//    16 px, the taskbar at 24 px and Start pins at 32 px, and a shrunk 512 px
//    design blurs. Tuned: 16, 20, 24, 32, 48 (assets/icon/icon-<size>.svg).
//    Scaled from icon.svg: 40, 64, 256.
//
// Each layer is stored as PNG, as `tauri icon` does; Windows Vista and later
// read PNG layers. No dependencies beyond the Tauri CLI.
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = join(import.meta.dirname, "..");
const SOURCES = join(ROOT, "assets", "icon");
const ICONS = join(ROOT, "src-tauri", "icons");
const TMP = join(ROOT, "target", "icons-tmp");

const TUNED = [16, 20, 24, 32, 48];
const SCALED = [40, 64, 256];
// Layer order in the .ico: 32 first like `tauri icon` (some readers take the first one).
const ORDER = [32, 16, 20, 24, 40, 48, 64, 256];

const TAURI_CLI = join(ROOT, "node_modules", "@tauri-apps", "cli", "tauri.js");

function tauri(...args) {
  const run = spawnSync(process.execPath, [TAURI_CLI, ...args], { cwd: ROOT, stdio: "inherit" });
  if (run.status !== 0) throw new Error(`tauri ${args.join(" ")} failed`);
}

function pngSize(png, file) {
  const signature = "89504e470d0a1a0a";
  if (png.subarray(0, 8).toString("hex") !== signature) throw new Error(`${file} is not a PNG`);
  return [png.readUInt32BE(16), png.readUInt32BE(20)];
}

/** An .ico file holding the given PNG images. */
function ico(images) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(images.length, 4);
  const entries = [];
  let offset = 6 + 16 * images.length;
  for (const { size, png } of images) {
    const entry = Buffer.alloc(16);
    entry.writeUInt8(size >= 256 ? 0 : size, 0); // 0 means 256
    entry.writeUInt8(size >= 256 ? 0 : size, 1);
    entry.writeUInt8(0, 2); // no palette
    entry.writeUInt8(0, 3);
    entry.writeUInt16LE(1, 4); // planes
    entry.writeUInt16LE(32, 6); // bits per pixel
    entry.writeUInt32LE(png.length, 8);
    entry.writeUInt32LE(offset, 12);
    entries.push(entry);
    offset += png.length;
  }
  return Buffer.concat([header, ...entries, ...images.map((i) => i.png)]);
}

tauri("icon", join(SOURCES, "icons.json"), "-o", ICONS);
// iOS is not a target (SPEC: Windows and Android).
rmSync(join(ICONS, "ios"), { recursive: true, force: true });

rmSync(TMP, { recursive: true, force: true });
mkdirSync(TMP, { recursive: true });
const layers = new Map();
for (const size of TUNED) {
  const out = join(TMP, `tuned-${size}`);
  tauri("icon", join(SOURCES, `icon-${size}.svg`), "-p", String(size), "-o", out);
  layers.set(size, join(out, `${size}x${size}.png`));
}
tauri("icon", join(SOURCES, "icon.svg"), "-p", SCALED.join(","), "-o", join(TMP, "scaled"));
for (const size of SCALED) layers.set(size, join(TMP, "scaled", `${size}x${size}.png`));

const images = ORDER.map((size) => {
  const file = layers.get(size);
  const png = readFileSync(file);
  const [w, h] = pngSize(png, file);
  if (w !== size || h !== size) throw new Error(`${file} is ${w}x${h}, expected ${size}x${size}`);
  return { size, png };
});
writeFileSync(join(ICONS, "icon.ico"), ico(images));
rmSync(TMP, { recursive: true, force: true });
console.log(
  `icon.ico: ${ORDER.map((s) => `${s}${TUNED.includes(s) ? " (tuned)" : ""}`).join(", ")}`,
);
