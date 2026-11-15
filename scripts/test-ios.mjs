import assert from "node:assert/strict";
import { join } from "node:path";

// Chromium with an iPhone viewport exercises production UI and Rust transport.
// This is deliberately not labelled a WKWebView or physical-device test.
export async function testIOS({ browser, dispatch, ui, output, report }) {
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true,
    colorScheme: "dark", userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 Version/18.0 Mobile/15E148 Safari/604.1" });
  const page = await context.newPage(), errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.route("**/__hpp_test_ipc", async route => route.fulfill({ contentType: "application/json", body: JSON.stringify(await dispatch(JSON.parse(route.request().postData()))) }));
  await page.addInitScript(() => { if (!localStorage.getItem("hpp.preferences")) localStorage.setItem("hpp.preferences", JSON.stringify({ autoNext: false })); });
  try {
    await page.goto(ui + "tests/plugins.html");
    await page.waitForFunction(() => document.querySelector("video")?.currentTime > .2);
    assert.equal(await page.locator("html").getAttribute("data-platform"), "ios");
    const nav = page.getByRole("navigation", { name: "主导航" });
    assert.equal(await nav.getByRole("button").count(), 4);
    for (const button of await nav.getByRole("button").all()) {
      const box = await button.boundingBox(); assert(box.width >= 44 && box.height >= 44);
    }
    await page.screenshot({ animations: "disabled", path: join(output, "ios-home-dark.png") });
    const touch = await context.newCDPSession(page);
    const tap = async () => {
      await touch.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: 150, y: 280 }] });
      await touch.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    };
    const videoBounds = await page.locator("video").boundingBox();
    await tap(); await nav.waitFor({ state: "detached" });
    assert.deepEqual(await page.locator("video").boundingBox(), videoBounds, "Hiding controls must not resize the video");
    await tap(); await nav.waitFor();
    await tap(); await tap(); await page.waitForFunction(() => document.querySelector("video")?.paused);
    await tap(); await tap(); await page.waitForFunction(() => !document.querySelector("video")?.paused);
    // Manual pause survives a background/foreground cycle; foreground playback
    // resumes only if the user had actually requested it.
    await page.locator("video").evaluate(video => video.pause());
    const visibility = hidden => page.evaluate(value => {
      Object.defineProperty(document, "hidden", { configurable: true, get: () => value });
      document.dispatchEvent(new Event("visibilitychange"));
    }, hidden);
    await visibility(true); await visibility(false);
    assert(await page.locator("video").evaluate(video => video.paused));
    await page.locator("video").evaluate(video => video.play());
    await page.waitForFunction(() => !document.querySelector("video")?.paused);
    await visibility(true); assert(await page.locator("video").evaluate(video => video.paused));
    await visibility(false); await page.waitForFunction(() => !document.querySelector("video")?.paused);
    await nav.getByRole("button", { name: "分类", exact: true }).click();
    await page.locator(".hpp-category-preview").first().waitFor();
    await page.screenshot({ animations: "disabled", path: join(output, "ios-category-dark.png") });
    await nav.getByRole("button", { name: "搜索", exact: true }).click();
    await page.getByRole("textbox", { name: "搜索所有插件" }).fill("城市");
    await page.getByRole("textbox", { name: "搜索所有插件" }).press("Enter");
    await page.waitForFunction(() => document.querySelectorAll(".legacy-search-poster").length === 2);
    for (const size of [{ width: 320, height: 680 }, { width: 844, height: 390 }, { width: 390, height: 844 }]) {
      await page.setViewportSize(size);
      const layout = await page.locator(".legacy-search-page").evaluate(element => ({ width: element.scrollWidth, viewport: innerWidth, overflow: [...element.querySelectorAll("*")].filter(e => e.getBoundingClientRect().right > innerWidth + 1).map(e => e.className).slice(0, 10) }));
      assert(layout.width <= layout.viewport, JSON.stringify({ size, ...layout }));
      const box = await nav.boundingBox(); assert(box.x >= 0 && box.x + box.width <= size.width && box.y + box.height <= size.height);
    }
    await page.screenshot({ animations: "disabled", path: join(output, "ios-search-dark.png") });
    await nav.getByRole("button", { name: "我的", exact: true }).click();
    await page.getByRole("button", { name: "设置", exact: true }).click();
    const appearance = page.getByRole("combobox", { name: "外观模式", exact: true });
    await appearance.selectOption("system");
    await page.emulateMedia({ colorScheme: "light" });
    await page.waitForFunction(() => document.documentElement.classList.contains("theme-light"));
    await page.screenshot({ animations: "disabled", path: join(output, "ios-settings-light.png") });
    await page.emulateMedia({ colorScheme: "dark" });
    await page.waitForFunction(() => !document.documentElement.classList.contains("theme-light"));
    await appearance.selectOption("light");
    await page.reload();
    await page.waitForFunction(() => document.documentElement.classList.contains("theme-light"));
    assert.equal(await page.evaluate(() => JSON.parse(localStorage.getItem("hpp.theme"))), "light");
    await nav.getByRole("button", { name: "我的", exact: true }).click();
    await page.getByRole("button", { name: "设置", exact: true }).click();
    await appearance.selectOption("dark");
    await page.waitForFunction(() => !document.documentElement.classList.contains("theme-light"));
    await page.screenshot({ animations: "disabled", path: join(output, "ios-settings-dark.png") });
    assert.deepEqual(errors, []);
    report.passed.push("iPhone viewport: real MP4 via Rust proxy, 44px four-tab navigation, manual pause and simulated lifecycle resume, search/category, 320px/landscape layout, persisted light/dark/system appearance (Chromium; native iOS APIs and notch require device)");
  } finally { await context.close(); }
}
