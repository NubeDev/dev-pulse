import { test, expect } from '@playwright/test';

test('Delete all wipes categories after confirm', async ({ page }) => {
  await page.goto('http://localhost:8732/login');
  await page.fill('input[type=email]', 'dev@dev.com');
  await page.fill('input[type=password]', 'dev123456789');
  await page.click('button[type=submit]');
  await page.waitForLoadState('networkidle');

  await page.goto('http://localhost:8732/#/projects/a246846c-9781-44f0-b289-8c5900cf3bdd?view=01747a23-a1b6-40d1-becc-0461a3426261');
  await expect(page.getByTestId('project-workbench-toolbar')).toBeVisible({ timeout: 15_000 });
  await page.getByTestId('project-manage-categories').click();
  await expect(page.getByTestId('categories-manager-dialog')).toBeVisible();

  // Sanity: there's at least one row to start.
  await expect(page.getByTestId('project-view-category-row-0')).toBeVisible();

  // Auto-accept the confirm.
  page.once('dialog', (d) => d.accept());
  await page.getByTestId('categories-manager-delete-all').click();

  // After accepting, all rows disappear and the Delete-all button
  // hides (categories.length === 0).
  await page.waitForTimeout(700);
  await expect(page.getByTestId('project-view-category-row-0')).toHaveCount(0);
  await expect(page.getByTestId('categories-manager-delete-all')).toHaveCount(0);

  // Restore the original three categories so the project state
  // doesn't drift between test runs.
  await page.getByTestId('project-view-category-chip-hardware').click();
  await page.getByTestId('project-view-category-chip-firmware').click();
  await page.getByTestId('project-view-category-chip-software').click();
  await page.waitForTimeout(700);
  await page.getByTestId('categories-manager-close').click();
});

test('Delete all cancel keeps the list', async ({ page }) => {
  await page.goto('http://localhost:8732/login');
  await page.fill('input[type=email]', 'dev@dev.com');
  await page.fill('input[type=password]', 'dev123456789');
  await page.click('button[type=submit]');
  await page.waitForLoadState('networkidle');

  await page.goto('http://localhost:8732/#/projects/a246846c-9781-44f0-b289-8c5900cf3bdd?view=01747a23-a1b6-40d1-becc-0461a3426261');
  await expect(page.getByTestId('project-workbench-toolbar')).toBeVisible({ timeout: 15_000 });
  await page.getByTestId('project-manage-categories').click();

  page.once('dialog', (d) => d.dismiss());
  await page.getByTestId('categories-manager-delete-all').click();
  await page.waitForTimeout(300);

  // Rows still there.
  await expect(page.getByTestId('project-view-category-row-0')).toBeVisible();
});
