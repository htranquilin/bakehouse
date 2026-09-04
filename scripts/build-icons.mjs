// Render visual-identity/app-icon.svg into the macOS .icns and the PNG set
// Tauri expects. Uses sharp (bundled libvips renders SVG) instead of librsvg,
// then macOS's iconutil for the .icns.
import sharp from "sharp";
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync, copyFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = join(root, "visual-identity", "app-icon.svg");
const out = join(root, "visual-identity", "build");
const iconset = join(out, "Bakehouse.iconset");
const icons = join(root, "src-tauri", "icons");

rmSync(out, { recursive: true, force: true });
mkdirSync(iconset, { recursive: true });
mkdirSync(icons, { recursive: true });

const render = (file, px) =>
  sharp(src, { density: (72 * px) / 1024 })
    .resize(px, px)
    .png()
    .toFile(file);

const members = [
  ["icon_16x16.png", 16], ["icon_16x16@2x.png", 32],
  ["icon_32x32.png", 32], ["icon_32x32@2x.png", 64],
  ["icon_128x128.png", 128], ["icon_128x128@2x.png", 256],
  ["icon_256x256.png", 256], ["icon_256x256@2x.png", 512],
  ["icon_512x512.png", 512], ["icon_512x512@2x.png", 1024],
];
await Promise.all(members.map(([name, px]) => render(join(iconset, name), px)));
execFileSync("iconutil", ["-c", "icns", iconset, "-o", join(out, "Bakehouse.icns")]);

// Tauri's expected names
copyFileSync(join(iconset, "icon_32x32.png"), join(icons, "32x32.png"));
copyFileSync(join(iconset, "icon_128x128.png"), join(icons, "128x128.png"));
copyFileSync(join(iconset, "icon_128x128@2x.png"), join(icons, "128x128@2x.png"));
copyFileSync(join(iconset, "icon_512x512.png"), join(icons, "icon.png"));
copyFileSync(join(out, "Bakehouse.icns"), join(icons, "icon.icns"));

console.log("Icons written to src-tauri/icons/ and visual-identity/build/");
