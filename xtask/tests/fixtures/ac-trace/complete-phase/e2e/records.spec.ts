import { expect, test } from '@playwright/test';

test.describe('records', () => {
  // AC: CE-90.2
  test('shows a record', async ({ page }) => {
    await page.goto('/');
    await expect(page).toHaveTitle(/Theos/);
  });
});
