import { mkdir, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { execFile } from "node:child_process";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { promisify } from "node:util";
import { chromium } from "playwright";

const execFileAsync = promisify(execFile);
const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const crateDir = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(crateDir, "..", "..");
const outputDir = path.resolve(
  process.argv[2] ?? "../../output/playwright/impulse-desktop-host-smoke",
);
const hostMode = process.argv[3] ?? "dioxus";
if (!["dioxus", "legacy-tauri"].includes(hostMode)) {
  throw new Error(
    `unsupported host mode ${hostMode}; expected dioxus or legacy-tauri`,
  );
}
const expectedHostKind = hostMode === "dioxus" ? "dioxus" : "legacy-tauri";
const fixturePath = path.join(outputDir, "host-readiness.html");

await mkdir(outputDir, { recursive: true });
await assertLocalAssets();
await writeFixture();

const { stdout: interopScript } = await execFileAsync(
  "cargo",
  ["run", "-q", "-p", "impulse-desktop", "--example", "emit_terminal_interop_script"],
  {
    cwd: path.join(repoRoot, "impulse-rs"),
    env: process.env,
    maxBuffer: 2 * 1024 * 1024,
  },
);

let dioxusHostBootstrap = "";
if (hostMode === "dioxus") {
  const { stdout } = await execFileAsync(
    "cargo",
    [
      "run",
      "-q",
      "-p",
      "impulse-desktop",
      "--features",
      "desktop-app",
      "--example",
      "emit_dioxus_host_bootstrap",
    ],
    {
      cwd: path.join(repoRoot, "impulse-rs"),
      env: process.env,
      maxBuffer: 2 * 1024 * 1024,
    },
  );
  dioxusHostBootstrap = extractScriptBody(stdout);
}

const browser = await chromium.launch({ headless: true });
try {
  const page = await browser.newPage({
    viewport: { width: 960, height: 540 },
    deviceScaleFactor: 1,
  });
  try {
    await page.addInitScript((mode) => {
      window.__IMPULSE_HOST_SMOKE = {
        invoked: [],
        listeners: {},
        unlistenCount: 0,
      };
      const hostApi = {
        invoke(command, payload) {
          window.__IMPULSE_HOST_SMOKE.invoked.push({ command, payload });
          return Promise.resolve(null);
        },
        listen(event, handler) {
          window.__IMPULSE_HOST_SMOKE.listeners[event] = handler;
          return Promise.resolve(() => {
            window.__IMPULSE_HOST_SMOKE.unlistenCount += 1;
          });
        },
      };
      window.__IMPULSE_TEST_HOST_API = hostApi;
      if (mode === "dioxus") {
        return;
      }
      window.__TAURI__ = {
        core: { invoke: hostApi.invoke },
        event: { listen: hostApi.listen },
      };
    }, hostMode);

    await page.goto(pathToFileURL(fixturePath).href, { waitUntil: "load" });
    await assertAssetsLoaded(page);
    if (hostMode === "dioxus") {
      await page.evaluate((bootstrap) => {
        window.eval(bootstrap);
        const bootstrapHost = window.__IMPULSE_DESKTOP_HOST;
        if (!bootstrapHost) {
          throw new Error("Dioxus host bootstrap did not install host adapter");
        }
        window.__IMPULSE_BOOTSTRAP_MANIFEST = {
          hostKind: bootstrapHost.hostKind,
          status: bootstrapHost.status,
          supportedInvokes: bootstrapHost.supportedInvokes,
          supportedEvents: bootstrapHost.supportedEvents,
        };
        window.__IMPULSE_BOOTSTRAP_PENDING_PROBE = {};
        const capturePending = async (label, call) => {
          try {
            await call();
            window.__IMPULSE_BOOTSTRAP_PENDING_PROBE[label] = "resolved";
          } catch (error) {
            window.__IMPULSE_BOOTSTRAP_PENDING_PROBE[label] = String(
              error?.message ?? error,
            );
          }
        };
        return Promise.all([
          capturePending("invoke", () => bootstrapHost.invoke("agent_snapshot")),
          capturePending("listen", () => bootstrapHost.listen("ops_update", () => {})),
        ]).then(() => {
          window.__IMPULSE_DESKTOP_HOST = {
            ...bootstrapHost,
            invoke: window.__IMPULSE_TEST_HOST_API.invoke,
            listen: window.__IMPULSE_TEST_HOST_API.listen,
          };
        });
      }, dioxusHostBootstrap);
      await assertDioxusBootstrapManifest(page);
      await assertPendingBootstrapFailsClosed(page);
    }

    const mounted = await page.evaluate((script) => {
      return window.eval(script);
    }, interopScript);
    assert(mounted === "mounted", `terminal interop returned ${mounted}`);
    await page.waitForFunction((mode) => {
      return document.documentElement.getAttribute("data-impulse-host-kind") === mode;
    }, expectedHostKind);

    await page.waitForFunction(() => {
      const smoke = window.__IMPULSE_HOST_SMOKE;
      return Boolean(smoke.listeners.terminal_output && smoke.listeners.terminal_exit);
    });
    await expectMountState(page, "mounted");

    await page.evaluate(() => {
      window.__impulseTerminalInterop.terminals.codex.focus();
    });
    await page.keyboard.type("x");
    await page.waitForFunction(() => {
      return window.__IMPULSE_HOST_SMOKE.invoked.some(
        (call) =>
          call.command === "agent_write" &&
          Array.isArray(call.payload?.request?.data) &&
          call.payload.request.data[0] === 120,
      );
    });

    await page.evaluate(() => {
      window.__impulseTerminalInterop.terminals.codex.resize(100, 30);
    });
    await page.waitForFunction(() => {
      return window.__IMPULSE_HOST_SMOKE.invoked.some(
        (call) =>
          call.command === "agent_resize" &&
          call.payload?.request?.session_id === "codex" &&
          call.payload.request.cols === 100 &&
          call.payload.request.rows === 30,
      );
    });

    await page.evaluate(() => {
      window.__IMPULSE_HOST_SMOKE.listeners.terminal_output({
        payload: { agent_id: "codex", data: [111, 107] },
      });
    });
    await page.waitForFunction(() => {
      const term = window.__impulseTerminalInterop.terminals.codex;
      return term?.buffer?.active?.getLine(0)?.translateToString(true).includes("ok");
    });

    await page.evaluate(() => {
      window.__IMPULSE_HOST_SMOKE.listeners.terminal_exit({
        payload: { agent_id: "codex" },
      });
    });
    await page.waitForFunction(() => {
      const term = window.__impulseTerminalInterop.terminals.codex;
      const rows = [];
      for (let i = 0; i < term.buffer.active.length; i += 1) {
        rows.push(term.buffer.active.getLine(i)?.translateToString(true) ?? "");
      }
      return rows.join("\n").includes("[process exited]");
    });

    await page.screenshot({
      path: path.join(outputDir, "host-readiness.png"),
      fullPage: false,
    });
  } finally {
    await page.close();
  }
} finally {
  await browser.close();
}

console.log(`${hostMode} host readiness smoke ok: ${fixturePath}`);

async function assertLocalAssets() {
  for (const asset of [
    "assets/vendor/xterm/xterm.css",
    "assets/vendor/xterm/xterm.js",
    "assets/vendor/xterm/addon-fit.js",
    "assets/vendor/xterm/manifest.json",
  ]) {
    const absolute = path.join(crateDir, asset);
    if (!existsSync(absolute)) {
      throw new Error(`missing local asset ${asset}; run npm run vendor:xterm`);
    }
  }
}

async function writeFixture() {
  const baseHref = pathToFileURL(`${crateDir}${path.sep}`).href;
  const html = `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <base href="${baseHref}">
    <link rel="stylesheet" href="assets/vendor/xterm/xterm.css">
    <script src="assets/vendor/xterm/xterm.js"></script>
    <script src="assets/vendor/xterm/addon-fit.js"></script>
    <style>
      html, body {
        width: 100%;
        height: 100%;
        margin: 0;
        background: #05080b;
      }
      body {
        display: grid;
        place-items: center;
      }
      [data-xterm-mount="true"] {
        width: 820px;
        height: 320px;
        border: 1px solid rgba(47, 208, 255, 0.45);
      }
    </style>
    <title>Impulse ${hostMode} host readiness smoke</title>
  </head>
  <body>
    <div
      id="terminal-pane-codex"
      data-xterm-mount="true"
      data-agent-id="codex"
      data-platform="codex"
      data-pty-owner="rust-backend"
      data-xterm-on-data="agent_write"
      data-xterm-on-resize="agent_resize"
    ></div>
  </body>
</html>
`;
  for (const forbidden of ["https://", "http://"]) {
    if (html.includes(forbidden)) {
      throw new Error(`host fixture contains remote asset ${forbidden}`);
    }
  }
  await writeFile(fixturePath, html);
}

async function assertAssetsLoaded(page) {
  const assets = await page.evaluate(() => ({
    terminal: Boolean(window.Terminal || window.XTerm?.Terminal),
    fitAddon: Boolean(window.FitAddon?.FitAddon || window.FitAddon),
  }));
  assert(assets.terminal, "local xterm.js did not set window.Terminal");
  assert(assets.fitAddon, "local addon-fit did not set window.FitAddon");
}

async function assertDioxusBootstrapManifest(page) {
  const manifest = await page.evaluate(() => window.__IMPULSE_BOOTSTRAP_MANIFEST);
  assert(manifest?.hostKind === "dioxus", `unexpected host kind ${manifest?.hostKind}`);
  assert(
    manifest?.status === "manifest-only-pending-dioxus-eval-bridge",
    `unexpected host status ${manifest?.status}`,
  );
  for (const command of ["agent_write", "agent_resize", "agent_snapshot", "mcp_invoke"]) {
    assert(
      manifest.supportedInvokes?.includes(command),
      `Dioxus host manifest missing invoke ${command}`,
    );
  }
  for (const event of ["terminal_output", "terminal_exit", "ops_update"]) {
    assert(
      manifest.supportedEvents?.includes(event),
      `Dioxus host manifest missing event ${event}`,
    );
  }
}

async function assertPendingBootstrapFailsClosed(page) {
  const probe = await page.evaluate(() => window.__IMPULSE_BOOTSTRAP_PENDING_PROBE);
  assert(
    probe?.invoke?.includes("Dioxus Desktop host adapter pending: invoke:agent_snapshot"),
    `pending Dioxus host invoke unexpectedly succeeded: ${probe?.invoke}`,
  );
  assert(
    probe?.listen?.includes("Dioxus Desktop host adapter pending: listen:ops_update"),
    `pending Dioxus host listen unexpectedly succeeded: ${probe?.listen}`,
  );
}

async function expectMountState(page, expected) {
  const state = await page.locator("[data-xterm-mount='true']").getAttribute("data-xterm-state");
  assert(state === expected, `expected mount state ${expected}, got ${state}`);
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function extractScriptBody(html) {
  const match = html.match(/<script>([\s\S]*)<\/script>/);
  if (!match) {
    throw new Error("Dioxus host bootstrap did not contain a script tag");
  }
  return match[1];
}
