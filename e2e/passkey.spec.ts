import { test, expect, Page } from "@playwright/test";

/**
 * Passkey E2E tests using real Touch ID via system Chrome.
 *
 * These run in headed mode against the system Google Chrome browser
 * so that macOS platform authenticator (Touch ID) prompts appear.
 * The user must physically tap Touch ID to proceed.
 *
 * Run with: npx playwright test --project=passkey --headed
 */
test.describe.serial("Passkey auth flow", () => {
  let page: Page;

  test.beforeAll(async ({ browser }) => {
    page = await browser.newPage();
  });

  test.afterAll(async () => {
    await page.close();
  });

  test("setup admin account", async () => {
    test.setTimeout(120_000);

    await page.goto("/auth/setup");
    await expect(page.locator("h1")).toHaveText("Create Admin Account");

    const setupFinished = page.waitForResponse(
      (resp) =>
        resp.url().includes("/auth/setup/finish") && resp.status() === 200,
      { timeout: 120_000 },
    );

    await page.fill("#username", "admin");
    await page.click('button:has-text("Register Passkey")');

    // Touch ID prompt — user taps to approve.
    await setupFinished;

    // Wait for the JS redirect to home page.
    await page.waitForURL("**/", { timeout: 10_000 });

    // Verify session cookie was set
    const cookies = await page.context().cookies();
    const sessionCookie = cookies.find((c) => c.name === "salita_session");
    expect(sessionCookie).toBeDefined();

    // Verify setup prompt is gone (user was created)
    await expect(page.locator("text=First-time Setup")).not.toBeVisible();
  });

  test("home page shows user exists", async () => {
    await page.goto("/");

    await expect(page.locator("text=First-time Setup")).not.toBeVisible();
    await expect(page.locator('a:has-text("View Stream")')).toBeVisible();
  });

  test("logout clears session", async () => {
    await page.goto("/");
    await page.evaluate(() =>
      fetch("/auth/logout", { method: "POST", redirect: "follow" }),
    );

    // Verify session cookie was cleared
    await page.waitForTimeout(200);
    const cookies = await page.context().cookies();
    const sessionCookie = cookies.find((c) => c.name === "salita_session");
    expect(!sessionCookie || sessionCookie.value === "").toBeTruthy();
  });

  test("login with passkey", async () => {
    test.setTimeout(120_000);

    await page.goto("/auth/login");
    await expect(page.locator("h1")).toHaveText("Sign In");

    const loginFinished = page.waitForResponse(
      (resp) =>
        resp.url().includes("/auth/login/finish") && resp.status() === 200,
      { timeout: 120_000 },
    );

    await page.click('button:has-text("Sign in with Passkey")');

    // Touch ID prompt — user taps to approve.
    await loginFinished;

    // Wait for the JS redirect to home page.
    await page.waitForURL("**/", { timeout: 10_000 });

    // Verify session cookie was set
    const cookies = await page.context().cookies();
    const sessionCookie = cookies.find((c) => c.name === "salita_session");
    expect(sessionCookie).toBeDefined();
    expect(sessionCookie!.value).toBeTruthy();
  });
});
