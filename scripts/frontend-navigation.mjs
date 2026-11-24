export async function selectCategorySource(page, name) {
  await page.getByRole("navigation", { name: "主导航" }).getByRole("button", { name: "分类", exact: true }).click();
  await page.getByRole("navigation", { name: "分类片源" }).getByRole("button", { name, exact: true }).click();
}
export async function openRecommendations(page, name) {
  await selectCategorySource(page, name);
  await page.getByRole("button", { name: `推荐分区 · ${name}`, exact: true }).click();
}
export const installedTab = page => page.getByRole("navigation", { name: "插件中心分栏" }).getByRole("button", { name: /^已安装/ }).click();
export const importTab = page => page.getByRole("navigation", { name: "插件中心分栏" }).getByRole("button", { name: /^导入与(列表)?订阅$/ }).click();

export async function playerPanel(page, panel) {
  const heading = page.locator('.legacy-panel-heading');
  if (await heading.count()) await heading.getByRole('button', {name: panel, exact: true}).click();
  else if (panel === '详情') await page.getByRole('button', {name: /查看详情/}).click();
  else await page.getByRole('button', {name: panel, exact: true}).click();
}
export async function chooseEpisode(page, name) {
  await playerPanel(page, '选集');
  await page.getByRole('button', {name, exact: true}).click();
  await playerPanel(page, '播放设置');
}
