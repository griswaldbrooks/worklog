import { test, expect } from "@playwright/test";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Add an entry via the new-entry form and return to the index. */
async function addEntry(page, date: string, text: string) {
  await page.goto("/new");
  await page.fill("#date", date);
  await page.fill("#items", text);
  await page.click('button[type="submit"]');
  await page.waitForURL("/");
}

/**
 * Delete all entries whose data-original starts with the given prefix.
 * This ensures tests are idempotent across repeated runs.
 */
async function deleteEntriesWithPrefix(page, prefix: string) {
  await page.goto("/");
  const ids = await page.evaluate((pfx) => {
    return Array.from(document.querySelectorAll(`.entry-text[data-original^="${pfx}"]`))
      .map((el) => el.getAttribute("data-id"))
      .filter(Boolean);
  }, prefix);

  for (const id of ids) {
    await page.evaluate(
      (entryId) =>
        fetch(`/entries/${entryId}/delete`, { method: "POST" }),
      id
    );
  }
}

/** Find the first <li> containing an entry whose data-original starts with the given text. */
function entryLi(page, text: string) {
  return page
    .locator(`li:has(.entry-text[data-original^="${text}"])`)
    .first();
}

/** Find the first .entry-text span whose data-original starts with the given text. */
function entrySpan(page, text: string) {
  return page.locator(`.entry-text[data-original^="${text}"]`).first();
}

// ---------------------------------------------------------------------------
// Clean up test entries before each test
// ---------------------------------------------------------------------------

const TEST_PREFIXES = [
  "PW_SPACING_A",
  "PW_SPACING_B",
  "PW_MULTI",
  "PW_TRASH",
  "PW_BULLET",
  "PW_EDIT",
  "PW_MD",
];

test.beforeEach(async ({ page }) => {
  for (const prefix of TEST_PREFIXES) {
    await deleteEntriesWithPrefix(page, prefix);
  }
});

// ---------------------------------------------------------------------------
// 1. Index page loads
// ---------------------------------------------------------------------------

test("index page loads with status 200 and heading", async ({ page }) => {
  const response = await page.goto("/");
  expect(response?.status()).toBe(200);
  await expect(page.locator("h1")).toHaveText("worklog");
});

// ---------------------------------------------------------------------------
// 2. Single-line entry spacing — no extra gaps
// ---------------------------------------------------------------------------

test("single-line entries have compact spacing", async ({ page }) => {
  await addEntry(page, "Mar 10, 2026", "PW_SPACING_A");
  await addEntry(page, "Mar 10, 2026", "PW_SPACING_B");
  await page.goto("/");

  const first = entryLi(page, "PW_SPACING_A");
  const second = entryLi(page, "PW_SPACING_B");
  await expect(first).toBeVisible();
  await expect(second).toBeVisible();

  const firstBox = await first.boundingBox();
  const secondBox = await second.boundingBox();
  expect(firstBox).toBeTruthy();
  expect(secondBox).toBeTruthy();

  // Gap should be less than 15px (0.2rem + 0.2rem margins ≈ 6-7px at default font)
  const gap = Math.abs(secondBox!.y - (firstBox!.y + firstBox!.height));
  expect(gap).toBeLessThan(15);
});

// ---------------------------------------------------------------------------
// 3. Multi-line entry with sub-list
// ---------------------------------------------------------------------------

test("multi-line entry renders sub-list indented without extra gaps", async ({
  page,
}) => {
  const multiLineText =
    "PW_MULTI met with team\n\n- Sub item Alpha\n- Sub item Beta";
  await addEntry(page, "Mar 10, 2026", multiLineText);
  await page.goto("/");

  const entry = entrySpan(page, "PW_MULTI");
  await expect(entry).toContainText("PW_MULTI met with team");

  const subList = entry.locator("ul");
  await expect(subList).toBeVisible();
  await expect(subList.locator("li")).toHaveCount(2);

  // Sub-list should be indented relative to parent entry text
  const entryBox = await entry.boundingBox();
  const subListBox = await subList.boundingBox();
  expect(entryBox).toBeTruthy();
  expect(subListBox).toBeTruthy();
  expect(subListBox!.x).toBeGreaterThan(entryBox!.x);
});

// ---------------------------------------------------------------------------
// 4. Trash icon position — inline, visible on hover
// ---------------------------------------------------------------------------

test("trash icon is inline with entry and visible on hover", async ({
  page,
}) => {
  await addEntry(page, "Mar 10, 2026", "PW_TRASH check");
  await page.goto("/");

  const li = entryLi(page, "PW_TRASH");
  const actions = li.locator(".entry-actions");

  // Hidden by default
  await expect(actions).toHaveCSS("visibility", "hidden");

  // Visible on hover
  await li.hover();
  await expect(actions).toHaveCSS("visibility", "visible");

  // The trash icon should be on the same line as the entry text
  const entryBox = await li.locator(".entry-text").boundingBox();
  const actionsBox = await actions.boundingBox();
  expect(entryBox).toBeTruthy();
  expect(actionsBox).toBeTruthy();
  expect(actionsBox!.y).toBeLessThan(entryBox!.y + entryBox!.height);
});

// ---------------------------------------------------------------------------
// 5. Bullet markers — each entry has a disc bullet
// ---------------------------------------------------------------------------

test("top-level entries have bullet markers", async ({ page }) => {
  await addEntry(page, "Mar 10, 2026", "PW_BULLET entry");
  await page.goto("/");

  const li = entryLi(page, "PW_BULLET");
  await expect(li).toBeVisible();

  // The entry text should be indented from the li's left edge to leave room
  // for a bullet marker. With display:flex suppressing native markers, the
  // text starts flush-left — so no indentation means no visible bullet.
  const hasVisibleBullet = await li.evaluate((el) => {
    const style = window.getComputedStyle(el);
    const entryText = el.querySelector(".entry-text");
    if (!entryText) return false;

    // Method 1: ::before pseudo-element with bullet content
    const before = window.getComputedStyle(el, "::before");
    if (
      before.content &&
      before.content !== "none" &&
      before.content !== '""' &&
      before.content !== '"normal"'
    ) {
      return true;
    }

    // Method 2: native list-style works only when NOT display:flex
    if (style.listStyleType === "disc" && style.display !== "flex") {
      return true;
    }

    // Method 3: check if the entry text is offset from li left edge,
    // indicating a visible marker or bullet pseudo-element
    const liRect = el.getBoundingClientRect();
    const textRect = entryText.getBoundingClientRect();
    const indent = textRect.left - liRect.left;
    // A visible bullet typically creates 10+ px of indentation
    if (indent > 10) {
      return true;
    }

    return false;
  });

  expect(hasVisibleBullet).toBe(true);
});

// ---------------------------------------------------------------------------
// 6. New entry form
// ---------------------------------------------------------------------------

test("new entry form has date, textarea, and hint text", async ({ page }) => {
  await page.goto("/new");

  await expect(page.locator("h1")).toHaveText("New Entry");
  await expect(page.locator("#date")).toBeVisible();
  await expect(page.locator("#items")).toBeVisible();
  await expect(page.locator("#items")).toHaveAttribute(
    "placeholder",
    "What did you work on?"
  );
  await expect(page.locator(".hint").first()).toContainText("Format:");
  await expect(page.locator('button[type="submit"]')).toHaveText("Save");
});

// ---------------------------------------------------------------------------
// 7. Inline edit — click opens textarea, Escape cancels
// ---------------------------------------------------------------------------

test("clicking entry opens textarea for editing", async ({ page }) => {
  await addEntry(page, "Mar 10, 2026", "PW_EDIT target");
  await page.goto("/");

  const entry = entrySpan(page, "PW_EDIT");
  await entry.click();

  // A textarea should appear inside the entry-text span
  const textarea = entry.locator("textarea.entry-edit-area");
  await expect(textarea).toBeVisible();
  await expect(textarea).toHaveValue("PW_EDIT target");

  // Escape should cancel and restore original HTML
  await textarea.press("Escape");
  await expect(entry.locator("textarea")).toHaveCount(0);
  await expect(entry).toContainText("PW_EDIT target");
});

// ---------------------------------------------------------------------------
// 8. Markdown rendering — links and bold
// ---------------------------------------------------------------------------

test("markdown renders links and bold text", async ({ page }) => {
  await addEntry(
    page,
    "Mar 10, 2026",
    "PW_MD reviewed **important** PR [#42](https://example.com/42)"
  );
  await page.goto("/");

  const entry = entrySpan(page, "PW_MD");
  await expect(entry.locator("strong")).toHaveText("important");

  const link = entry.locator("a");
  await expect(link).toHaveText("#42");
  await expect(link).toHaveAttribute("href", "https://example.com/42");
});
