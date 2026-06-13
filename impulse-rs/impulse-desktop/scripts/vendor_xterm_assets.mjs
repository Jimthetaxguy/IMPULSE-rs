import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const crateDir = path.resolve(scriptDir, "..");
const vendorDir = path.join(crateDir, "assets", "vendor", "xterm");

const assets = [
  {
    packageName: "@xterm/xterm",
    packageJson: "node_modules/@xterm/xterm/package.json",
    source: "node_modules/@xterm/xterm/css/xterm.css",
    target: "assets/vendor/xterm/xterm.css",
    provides: "xterm.css",
  },
  {
    packageName: "@xterm/xterm",
    packageJson: "node_modules/@xterm/xterm/package.json",
    source: "node_modules/@xterm/xterm/lib/xterm.js",
    target: "assets/vendor/xterm/xterm.js",
    provides: "window.Terminal",
  },
  {
    packageName: "@xterm/addon-fit",
    packageJson: "node_modules/@xterm/addon-fit/package.json",
    source: "node_modules/@xterm/addon-fit/lib/addon-fit.js",
    target: "assets/vendor/xterm/addon-fit.js",
    provides: "window.FitAddon.FitAddon",
  },
  {
    packageName: "@xterm/xterm",
    packageJson: "node_modules/@xterm/xterm/package.json",
    source: "node_modules/@xterm/xterm/LICENSE",
    target: "assets/vendor/xterm/LICENSE.xterm.txt",
    provides: "license",
  },
  {
    packageName: "@xterm/addon-fit",
    packageJson: "node_modules/@xterm/addon-fit/package.json",
    source: "node_modules/@xterm/addon-fit/LICENSE",
    target: "assets/vendor/xterm/LICENSE.addon-fit.txt",
    provides: "license",
  },
];

await mkdir(vendorDir, { recursive: true });

const packageVersions = {};
for (const asset of assets) {
  const packagePath = path.join(crateDir, asset.packageJson);
  const packageMeta = JSON.parse(await readFile(packagePath, "utf8"));
  packageVersions[asset.packageName] = packageMeta.version;
  await copyFile(path.join(crateDir, asset.source), path.join(crateDir, asset.target));
}

const manifest = {
  generatedBy: "scripts/vendor_xterm_assets.mjs",
  source: "npm",
  packages: packageVersions,
  globals: {
    terminal: "window.Terminal",
    fitAddon: "window.FitAddon.FitAddon",
  },
  assets: assets.map(({ packageName, target, provides }) => ({
    package: packageName,
    path: target,
    provides,
  })),
};

await writeFile(
  path.join(vendorDir, "manifest.json"),
  `${JSON.stringify(manifest, null, 2)}\n`,
);

console.log(
  `vendored xterm assets: ${Object.entries(packageVersions)
    .map(([name, version]) => `${name}@${version}`)
    .join(", ")}`,
);
