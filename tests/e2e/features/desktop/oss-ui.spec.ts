import { expect, test } from "../../core/fixtures/base.fixture";
import type { Page, TestInfo } from "@playwright/test";

const desktopPresentation = process.env.BIFROST_E2E_DESKTOP_UI === "1";

const retainedNavigation = [
  { title: "LLM Logs", href: "/workspace/logs" },
  { title: "Model Providers", href: "/workspace/providers" },
  { title: "Routing Rules", href: "/workspace/routing-rules" },
  { title: "Complexity Router", href: "/workspace/complexity-router" },
  { title: "Virtual Keys", href: "/workspace/governance/virtual-keys" },
  { title: "Teams", href: "/workspace/governance/teams" },
  { title: "Customers", href: "/workspace/governance/customers" },
  { title: "API Keys", href: "/workspace/config/api-keys" },
  { title: "MCP Catalog", href: "/workspace/mcp-registry" },
  { title: "Prompt Repository", href: "/workspace/prompt-repo" },
];

// These are every enterprise-only top-level entry and sub-entry in the app sidebar.
const hiddenNavigation = [
  { title: "Circuit Breaker", href: "/workspace/circuit-breaker" },
  { title: "Alerting", href: "/workspace/alerting" },
  { title: "Channels", href: "/workspace/alerting/channels" },
  { title: "Rules", href: "/workspace/alerting/rules" },
  { title: "History", href: "/workspace/alerting/history" },
  { title: "Users", href: "/workspace/governance/users" },
  { title: "Business Units", href: "/workspace/governance/business-units" },
  { title: "User Provisioning", href: "/workspace/scim" },
  { title: "Roles & Permissions", href: "/workspace/governance/rbac" },
  { title: "Access Profiles", href: "/workspace/governance/access-profiles" },
  { title: "Projects", href: "/workspace/governance/projects" },
  { title: "Audit Logs", href: "/workspace/audit-logs" },
  { title: "Guardrails", href: "/workspace/guardrails" },
  { title: "Rules", href: "/workspace/guardrails/configuration" },
  { title: "Providers", href: "/workspace/guardrails/providers" },
  { title: "Edge Control", href: "/workspace/edge-control" },
  { title: "Devices", href: "/workspace/edge-control/devices" },
  { title: "Approvals", href: "/workspace/edge-control/inventory" },
  { title: "Edge Settings", href: "/workspace/edge-control/config" },
  { title: "Cluster Config", href: "/workspace/cluster" },
  { title: "Adaptive Routing", href: "/workspace/adaptive-routing" },
  { title: "Dashboard", href: "/workspace/adaptive-routing" },
  { title: "Settings", href: "/workspace/adaptive-routing/settings" },
];

const hiddenConnectors = ["datadog", "bigquery", "kafka", "pubsub", "splunk"];
const navigationGroups = new Set(["Alerting", "Guardrails", "Edge Control", "Adaptive Routing"]);

async function capture(page: Page, testInfo: TestInfo, name: string) {
  const reminder = page.getByRole("button", { name: "Close for now", exact: true });
  if (await reminder.isVisible()) await reminder.click();
  await page.screenshot({ path: testInfo.outputPath(`${name}.png`), fullPage: true });
}

test.use({ skipAutoLogin: true });

test.describe("desktop OSS UI presentation", () => {
  test.beforeEach(async ({ page }) => {
    const reminder = page.getByRole("button", { name: "Close for now", exact: true });
    await page.addLocatorHandler(reminder, async () => {
      await reminder.click();
    });
  });
  test("filters every audited sidebar entry before search and retains OSS navigation", async ({
    page,
  }, testInfo) => {
    await page.goto("/workspace");
    const sidebar = page.locator('[data-sidebar="sidebar"]');
    const search = sidebar.getByRole("textbox", { name: "Search sidebar navigation" });
    await expect(sidebar).toBeVisible();

    for (const entry of retainedNavigation) {
      await search.fill(entry.title);
      await expect(sidebar.locator(`a[href="${entry.href}"]`).first()).toBeVisible();
    }

    for (const entry of hiddenNavigation) {
      await search.fill(entry.title);
      const result = navigationGroups.has(entry.title)
        ? sidebar.getByRole("button", { name: entry.title, exact: true })
        : sidebar.locator(`a[href="${entry.href}"]`);
      if (desktopPresentation) await expect(result).toHaveCount(0);
      else await expect(result.first()).toBeVisible();
    }

    // Keyboard selection must use the filtered result list, not the unfiltered sidebar list.
    await search.fill("Complexity Router");
    await search.press("ArrowDown");
    await search.press("Enter");
    await expect(page).toHaveURL(/\/workspace\/complexity-router$/);
    await capture(page, testInfo, "sidebar-search-and-keyboard-navigation");
  });

  test("filters only audited observability selectors and leaves old direct routes renderable", async ({
    page,
  }, testInfo) => {
    await page.goto("/workspace/observability?plugin=datadog");
    for (const id of ["otel", "prometheus", "maxim", "newrelic"]) {
      await expect(page.getByTestId(`observability-provider-btn-${id}`)).toBeVisible();
    }
    for (const id of hiddenConnectors) {
      const selector = page.getByTestId(`observability-provider-btn-${id}`);
      if (desktopPresentation) await expect(selector).toHaveCount(0);
      else await expect(selector).toBeVisible();
    }
    await expect(page).toHaveURL(/\/workspace\/observability\?plugin=datadog/);
    await capture(page, testInfo, "observability-connectors");
  });

  test("keeps API key auth guidance without an enterprise promotion and neutralizes placeholder routes", async ({
    page,
  }, testInfo) => {
    await page.goto("/workspace/config/api-keys");
    await expect(page.getByText(/admin username and password|Basic auth/i).first()).toBeVisible();
    if (desktopPresentation) {
      await expect(
        page.getByRole("button", { name: /book a demo|read more|contact sales/i }),
      ).toHaveCount(0);
    }

    await page.goto("/workspace/circuit-breaker");
    if (desktopPresentation) {
      await expect(
        page.getByText("This feature is unavailable in the desktop application."),
      ).toBeVisible();
      await expect(page.getByRole("button", { name: /book a demo|read more/i })).toHaveCount(0);
    } else {
      await expect(page.getByRole("button", { name: /book a demo/i })).toBeVisible();
    }
    await capture(page, testInfo, "auth-guidance-and-placeholder-route");
  });

  test("keeps a real prompt editor and settings while desktop omits deployments", async ({
    page,
  }, testInfo) => {
    const name = `desktop-oss-ui-${Date.now()}`;
    const createResponse = await page.request.post("/api/prompt-repo/prompts", { data: { name } });
    expect(createResponse.status(), await createResponse.text()).toBe(200);
    const { prompt } = (await createResponse.json()) as { prompt: { id: string } };
    expect(prompt.id).toBeTruthy();

    try {
      await page.goto(`/workspace/prompt-repo?promptId=${encodeURIComponent(prompt.id)}`);
      await expect(page.getByTestId(`sidebar-prompt-${prompt.id}`)).toBeVisible();
      await expect(page.getByTestId("new-message-textarea")).toBeVisible();
      await page.getByTestId("new-message-textarea").fill("Disposable desktop UI smoke message");
      await expect(page.getByTestId("new-message-textarea")).toHaveValue(
        "Disposable desktop UI smoke message",
      );
      await expect(page.getByTestId("prompts-configuration-trigger")).toBeVisible();
      await expect(page.getByTestId("settings-provider")).toBeVisible();
      if (desktopPresentation)
        await expect(page.getByText("Deployments", { exact: true })).toHaveCount(0);
      else await expect(page.getByText("Deployments", { exact: true })).toBeVisible();
      await capture(page, testInfo, "prompt-editor-and-settings");
    } finally {
      const deleteResponse = await page.request.delete(
        `/api/prompt-repo/prompts/${encodeURIComponent(prompt.id)}`,
      );
      expect(deleteResponse.status(), await deleteResponse.text()).toBe(200);
    }
  });

  test("filters desktop Enterprise flag rows without changing OSS rows", async ({
    page,
  }, testInfo) => {
    await page.route("**/api/feature-flags**", async (route) => {
      await route.fulfill({
        contentType: "application/json",
        body: JSON.stringify({
          flags: [
            {
              id: "desktop-enterprise-only",
              display_name: "Enterprise test flag",
              enabled: false,
              locked: true,
              registered: true,
              enterprise_only: true,
              source: "default",
            },
            {
              id: "desktop-oss",
              display_name: "OSS test flag",
              enabled: true,
              locked: false,
              registered: true,
              enterprise_only: false,
              source: "default",
            },
          ],
        }),
      });
    });
    await page.goto("/workspace/config/feature-flags");
    await expect(page.getByText("OSS test flag")).toBeVisible();
    if (desktopPresentation) await expect(page.getByText("Enterprise test flag")).toHaveCount(0);
    else await expect(page.getByText("Enterprise test flag")).toBeVisible();
    await capture(page, testInfo, "feature-flag-filtering");
  });
});