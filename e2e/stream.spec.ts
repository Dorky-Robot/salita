import { test, expect, Page } from "@playwright/test";

/** Seed a test user and set the session cookie on the page. */
async function seedAndLogin(page: Page) {
  const resp = await page.request.get("/test/seed");
  expect(resp.ok()).toBeTruthy();
}

test.describe("Stream page (unauthenticated)", () => {
  test("renders stream heading", async ({ page }) => {
    await page.goto("/stream");
    await expect(page.locator("h1")).toHaveText("Stream");
  });

  test("shows sign-in prompt when not logged in", async ({ page }) => {
    await page.goto("/stream");
    await expect(page.locator('a[href="/auth/login"]')).toBeVisible();
  });

  test("shows empty state when no posts", async ({ page }) => {
    await page.goto("/stream");
    await expect(page.locator("text=No posts yet")).toBeVisible();
  });
});

test.describe("Stream page (authenticated)", () => {
  test.beforeEach(async ({ page }) => {
    await seedAndLogin(page);
  });

  test("shows compose form when logged in", async ({ page }) => {
    await page.goto("/stream");
    await expect(page.locator('textarea[name="body"]')).toBeVisible();
    await expect(page.locator('button:has-text("Post")')).toBeVisible();
  });

  test("create post appears in feed", async ({ page }) => {
    await page.goto("/stream");
    await page.fill('textarea[name="body"]', "Hello from E2E test!");
    await page.click('button:has-text("Post")');

    // Wait for HTMX to insert the post
    await expect(page.locator("text=Hello from E2E test!")).toBeVisible({
      timeout: 5000,
    });
  });

  test("post persists across page reload", async ({ page }) => {
    await page.goto("/stream");
    await page.fill('textarea[name="body"]', "Persistent post");
    await page.click('button:has-text("Post")');
    await expect(page.locator("text=Persistent post")).toBeVisible({
      timeout: 5000,
    });

    await page.reload();
    await expect(page.locator("text=Persistent post")).toBeVisible();
  });

  test("delete post removes it from feed", async ({ page }) => {
    await page.goto("/stream");
    await page.fill('textarea[name="body"]', "Post to delete");
    await page.click('button:has-text("Post")');
    await expect(page.locator("text=Post to delete")).toBeVisible({
      timeout: 5000,
    });

    // Accept the confirmation dialog
    page.on("dialog", (dialog) => dialog.accept());
    await page.click('button:has-text("delete")');

    await expect(page.locator("text=Post to delete")).not.toBeVisible({
      timeout: 5000,
    });
  });

  test("reaction toggle updates count", async ({ page }) => {
    await page.goto("/stream");
    await page.fill('textarea[name="body"]', "React to this");
    await page.click('button:has-text("Post")');
    await expect(page.locator("text=React to this")).toBeVisible({
      timeout: 5000,
    });

    // Find the like button (first reaction button in the new post)
    const post = page.locator("article").first();
    const likeBtn = post.locator('button[hx-vals*="like"]');
    await likeBtn.click();

    // After toggling, the like count should show "1"
    await expect(post.locator("text=1")).toBeVisible({ timeout: 5000 });
  });

  test("add comment appears under post", async ({ page }) => {
    await page.goto("/stream");
    await page.fill('textarea[name="body"]', "Comment on this");
    await page.click('button:has-text("Post")');
    await expect(page.locator("text=Comment on this")).toBeVisible({
      timeout: 5000,
    });

    // Wait for comment section to lazy-load
    const post = page.locator("article").first();
    const commentInput = post.locator('input[name="body"]');
    await expect(commentInput).toBeVisible({ timeout: 5000 });

    await commentInput.fill("Great post!");
    await post.locator('button:has-text("Reply")').click();

    await expect(page.locator("text=Great post!")).toBeVisible({
      timeout: 5000,
    });
  });
});
