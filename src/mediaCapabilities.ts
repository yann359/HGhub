export type AppleVideo = HTMLVideoElement & {
  webkitEnterFullscreen?: () => void;
  webkitSupportsPresentationMode?: (mode: string) => boolean;
  webkitSetPresentationMode?: (mode: string) => void;
  webkitPresentationMode?: string;
};
export function inPictureInPicture(video: AppleVideo) {
  return document.pictureInPictureElement === video || video.webkitPresentationMode === "picture-in-picture";
}
export async function fullscreen(video: AppleVideo, container: HTMLElement) {
  if (video.webkitEnterFullscreen) video.webkitEnterFullscreen();
  else if (container.requestFullscreen) await container.requestFullscreen();
  else throw new Error("当前系统不支持视频全屏");
}
export async function pictureInPicture(video: AppleVideo) {
  if (video.webkitSupportsPresentationMode?.("picture-in-picture")) {
    video.webkitSetPresentationMode?.(video.webkitPresentationMode === "picture-in-picture" ? "inline" : "picture-in-picture");
  } else if (document.pictureInPictureElement) await document.exitPictureInPicture();
  else if (document.pictureInPictureEnabled && video.requestPictureInPicture) await video.requestPictureInPicture();
  else throw new Error("当前系统或视频不支持画中画");
}
