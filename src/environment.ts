export const APP_VERSION = __APP_VERSION__;
const ua = navigator.userAgent;
export const platform = /Android/i.test(ua) ? "android" : /iPhone|iPad|iPod/i.test(ua) || /Macintosh/i.test(ua) && navigator.maxTouchPoints > 1 ? "ios" : /Macintosh|Mac OS X/i.test(ua) ? "macos" : /Linux/i.test(ua) ? "linux" : "windows";
export const isMobile = platform === "android" || platform === "ios";

document.documentElement.dataset.platform = platform;
