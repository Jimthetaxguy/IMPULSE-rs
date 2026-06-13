import { mkdir, readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const crateDir = path.resolve(scriptDir, "..");
const fixtureDir = path.resolve(
  process.argv[2] ?? "../../output/playwright/impulse-desktop-visual",
);
const outputDir = path.resolve(process.argv[3] ?? fixtureDir);
const viewports = [
  { label: "desktop", width: 1440, height: 900 },
  { label: "compact", width: 1024, height: 768 },
];
const routes = [
  {
    slug: "terminal",
    active: ".view-terminal.active",
    required: '.view-terminal.active [data-xterm-mount="true"]',
    visibleText: "Codex Live",
  },
  {
    slug: "memory",
    active: ".view-memory.active",
    visibleText: "Context health",
  },
  {
    slug: "review",
    active: ".review-console",
    visibleText: "Review Queue",
  },
  {
    slug: "artifacts",
    active: ".view-artifacts.active",
    visibleText: "Artifacts",
  },
  {
    slug: "supervisor",
    active: '[data-source="operator_board"]',
    visibleText: "In flight",
  },
];

await mkdir(outputDir, { recursive: true });

for (const route of routes) {
  const html = await readFile(path.join(fixtureDir, `${route.slug}.html`), "utf8");
  for (const forbidden of ["fonts.googleapis", "fonts.gstatic", "https://", "http://"]) {
    if (html.includes(forbidden)) {
      throw new Error(`${route.slug}: fixture contains remote asset ${forbidden}`);
    }
  }
  for (const assetPath of html.matchAll(/(?:src|href)="(assets\/vendor\/xterm\/[^"]+)"/g)) {
    const localPath = path.join(crateDir, assetPath[1]);
    if (!existsSync(localPath)) {
      throw new Error(`${route.slug}: missing local terminal asset ${assetPath[1]}`);
    }
  }
}

const browser = await chromium.launch({ headless: true });
try {
  for (const viewport of viewports) {
    const page = await browser.newPage({
      viewport: { width: viewport.width, height: viewport.height },
      deviceScaleFactor: 1,
    });
    try {
      for (const route of routes) {
        const fileUrl = pathToFileURL(path.join(fixtureDir, `${route.slug}.html`)).href;
        await page.goto(fileUrl, { waitUntil: "load" });
        await page.waitForSelector(".impulse-shell");
        await assertTerminalAssetsLoaded(page, route.slug);
        await assertLayout(page, route, viewport);
        await page.screenshot({
          path: path.join(outputDir, `${route.slug}-${viewport.width}x${viewport.height}.png`),
          fullPage: false,
        });
      }
    } finally {
      await page.close();
    }
  }
} finally {
  await browser.close();
}

console.log(
  `visual smoke ok: ${routes.length} routes x ${viewports.length} viewports in ${outputDir}`,
);

async function assertLayout(page, route, viewport) {
  await expectVisible(page, route.active, `${route.slug} active route selector`);
  if (route.required) {
    await expectVisible(page, route.required, `${route.slug} required route selector`);
  }
  await expectTextVisible(page, route.visibleText, route.slug);

  const result = await page.evaluate((routeSlug) => {
    const rectOf = (selector) => {
      const element = document.querySelector(selector);
      if (!element) return null;
      const rect = element.getBoundingClientRect();
      return {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
        left: rect.left,
      };
    };
    const text = document.body.innerText || "";
    return {
      routeSlug,
      bodyTextLength: text.trim().length,
      shell: rectOf(".impulse-shell"),
      top: rectOf(".top-bar"),
      grid: rectOf(".workspace-grid"),
      left: rectOf(".left-rail"),
      stage: rectOf(".terminal-stage"),
      inspector: rectOf(".right-inspector"),
      footer: rectOf(".event-strip"),
      scrollWidth: document.documentElement.scrollWidth,
      scrollHeight: document.documentElement.scrollHeight,
      clientWidth: document.documentElement.clientWidth,
      clientHeight: document.documentElement.clientHeight,
      terminalVisible: rectOf(".view-terminal.active"),
    };
  }, route.slug);

  for (const key of ["shell", "top", "grid", "left", "stage", "inspector", "footer"]) {
    const rect = result[key];
    assert(rect, `${route.slug}: missing ${key}`);
    assert(rect.width > 0, `${route.slug}: ${key} width is zero`);
    assert(rect.height > 0, `${route.slug}: ${key} height is zero`);
  }

  assert(result.bodyTextLength > 450, `${route.slug}: body text too short`);
  assert(Math.abs(result.shell.left) <= 1, `${route.slug}: shell left not aligned`);
  assert(Math.abs(result.shell.top) <= 1, `${route.slug}: shell top not aligned`);
  assert(
    Math.abs(result.shell.width - viewport.width) <= 1,
    `${route.slug}: shell width ${result.shell.width} != ${viewport.width}`,
  );
  assert(
    Math.abs(result.shell.height - viewport.height) <= 1,
    `${route.slug}: shell height ${result.shell.height} != ${viewport.height}`,
  );
  assert(
    result.scrollWidth <= result.clientWidth + 1,
    `${route.slug}: horizontal overflow ${result.scrollWidth} > ${result.clientWidth}`,
  );
  assert(
    result.scrollHeight <= result.clientHeight + 1,
    `${route.slug}: vertical overflow ${result.scrollHeight} > ${result.clientHeight}`,
  );
  assert(result.top.bottom <= result.grid.top + 1, `${route.slug}: top overlaps grid`);
  assert(result.grid.bottom <= result.footer.top + 1, `${route.slug}: grid overlaps footer`);
  assert(result.left.right <= result.stage.left + 1, `${route.slug}: left rail overlaps stage`);
  assert(
    result.stage.right <= result.inspector.left + 1,
    `${route.slug}: stage overlaps inspector`,
  );

  if (route.slug !== "terminal") {
    assert(!result.terminalVisible, `${route.slug}: terminal route should not be active`);
  }
}

async function assertTerminalAssetsLoaded(page, routeSlug) {
  const assets = await page.evaluate(() => ({
    terminal: Boolean(window.Terminal || window.XTerm?.Terminal),
    fitAddon: Boolean(window.FitAddon?.FitAddon || window.FitAddon),
  }));
  assert(assets.terminal, `${routeSlug}: local xterm.js did not set window.Terminal`);
  assert(assets.fitAddon, `${routeSlug}: local addon-fit did not set window.FitAddon`);
}

async function expectVisible(page, selector, label) {
  const locator = page.locator(selector).first();
  if ((await locator.count()) === 0) {
    throw new Error(`missing ${label}: ${selector}`);
  }
  const box = await locator.boundingBox();
  if (!box || box.width <= 0 || box.height <= 0) {
    throw new Error(`not visible ${label}: ${selector}`);
  }
}

async function expectTextVisible(page, text, routeSlug) {
  const locator = page.getByText(text, { exact: false }).first();
  if ((await locator.count()) === 0) {
    throw new Error(`${routeSlug}: missing visible text ${text}`);
  }
  const box = await locator.boundingBox();
  if (!box || box.width <= 0 || box.height <= 0) {
    throw new Error(`${routeSlug}: text is not visible ${text}`);
  }
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
