import { test, expect } from "@playwright/test";

test.describe("Welcome page", () => {
  test("renders the welcome heading", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("h1")).toHaveText("Welcome to Salita");
  });

  test("shows first-time setup prompt when no users exist", async ({
    page,
  }) => {
    await page.goto("/");
    await expect(page.locator("text=First-time Setup")).toBeVisible();
    await expect(page.locator("text=Set Up Admin Account")).toBeVisible();
  });

  test("setup link points to /auth/setup", async ({ page }) => {
    await page.goto("/");
    const link = page.locator('a:has-text("Set Up Admin Account")');
    await expect(link).toHaveAttribute("href", "/auth/setup");
  });

  test("nav links are present", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator('nav a[href="/stream"]')).toBeVisible();
    await expect(page.locator('nav a[href="/social"]')).toBeVisible();
    await expect(page.locator('nav a[href="/services"]')).toBeVisible();
  });

  test("page has correct title", async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveTitle("Welcome — Salita");
  });

  test("static assets load successfully", async ({ page }) => {
    const responses: Record<string, number> = {};

    page.on("response", (response) => {
      const url = new URL(response.url());
      if (url.pathname.startsWith("/assets/")) {
        responses[url.pathname] = response.status();
      }
    });

    await page.goto("/");

    // Wait a moment for deferred scripts to load
    await page.waitForTimeout(1000);

    expect(responses["/assets/css/output.css"]).toBe(200);
    expect(responses["/assets/js/htmx.min.js"]).toBe(200);
    expect(responses["/assets/js/app.js"]).toBe(200);
  });
});
