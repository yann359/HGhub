/** HPP 1.0 draft public contract. Development-host support is tracked in docs/plugin-protocol/IMPLEMENTATION.md. */
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json | undefined };
export type TransferValue = null | boolean | number | string | Uint8Array | TransferValue[] | { [key: string]: TransferValue | undefined };
export type ErrorCode = "AUTH_REQUIRED" | "AUTH_INVALID" | "AUTH_EXPIRED" | "NOT_FOUND" | "CONTENT_LOCKED" | "RATE_LIMITED" | "UPSTREAM_ERROR" | "UPSTREAM_CHANGED" | "INTERACTION_REQUIRED" | "NETWORK" | "TIMEOUT" | "CANCELLED" | "INVALID_ARGUMENT" | "UNSUPPORTED" | "INTERNAL" | "STORAGE_FULL";
export interface PluginFailure { code: ErrorCode; message: string; details?: Json; retryAfterSec?: number; retryable?: boolean }
export interface CallContext { readonly requestId: string; readonly deadline: number | null; readonly signal: AbortSignal }
export interface Setting {
  key: string; type: "text" | "password" | "switch" | "number" | "select"; label: string;
  description?: string; default?: string | boolean | number; placeholder?: string; min?: number; max?: number;
  options?: { value: string; label: string }[];
}
export interface PluginMeta {
  id: string; name: string; version: string; protocol: 1; kind?: "source" | "resolver";
  description?: string; author?: string; homepage?: string; updateUrl?: string; matches?: string[]; linkDomains?: string[];
  defaultHeaders?: Record<string, string>; timeouts?: Partial<Record<"read" | "playback" | "login" | "transform", number>>;
  features?: string[]; requires?: { type: "feature" | "api"; name: string }[];
  allowInsecureHttp?: boolean; mature?: boolean; settings?: Setting[];
}
export type MediaType = "movie" | "series" | "anime" | "variety" | "documentary" | "short" | "live" | "other";
export type Codec = "h264" | "hevc" | "av1";
export type MediaFormat = "hls" | "dash" | "progressive";
export type PlaybackReason = "play" | "preload" | "refresh";
export type ResourceTransform = { type: "aes-cbc"; key: string; iv: string; padding?: "pkcs7" | "none" } | { type: "plugin"; name: string; state?: Json };
export interface ResourceRef { url: string; headers?: Record<string, string>; transform?: ResourceTransform }
export type ImageRef = string | ResourceRef;
export interface Author { id: string; name: string; avatar?: ImageRef; description?: string; followers?: number }
export interface MediaItem {
  id: string; title: string; type?: MediaType; cover?: ImageRef; backdrop?: ImageRef; description?: string; tags?: string[];
  score?: number; year?: number; status?: "ongoing" | "finished" | "upcoming"; episodeCount?: number; totalEpisodes?: number;
  durationSec?: number; stats?: Partial<Record<"plays" | "hot" | "likes" | "favorites" | "comments", number>>;
  author?: Author; publishedAt?: number; updatedAt?: number; badge?: string; entryEpisodeId?: string;
}
export interface Episode { id: string; index: number; season?: number; episodeNumber?: number; label?: string; title?: string; durationSec?: number; airDate?: number; thumbnail?: ImageRef; locked?: boolean; lockReason?: string }
export interface Playlist { id: string; label?: string; kind?: "main" | "dub" | "extra" | "other"; episodes: Episode[]; next?: string | null }
export interface MediaDetail extends MediaItem {
  playlists: Playlist[]; seasons?: { number: number; label: string }[];
  people?: { name: string; role?: string; avatar?: ImageRef }[]; trailers?: Stream[];
  interaction?: { liked?: boolean; favorited?: boolean; likedEpisodeIds?: string[] }; related?: MediaItem[];
}
export interface PageParams { cursor?: string; limit?: number }
export interface Page<T> { items: T[]; next?: string | null; total?: number }
interface StreamBase {
  url: string; codec?: Codec; label?: string; height?: number; bitrate?: number; serverId?: string; live?: boolean;
  headers?: Record<string, string>; transforms?: Partial<Record<"playlist" | "segment" | "key", ResourceTransform>>;
}
export type Stream = StreamBase & (
  | { kind?: "media"; format: MediaFormat }
  | { kind: "page"; resolverId?: string; format?: never }
  | { kind: "external"; external?: { app?: string; mime?: string }; format?: never }
);
export interface PlaybackServer { id: string; label?: string }
export interface SubtitleTrack { url: string; format: "vtt" | "srt" | "ass"; language?: string; label?: string; default?: boolean; headers?: Record<string, string> }
export interface PlaybackInfo { streams: Stream[]; servers?: PlaybackServer[]; subtitles?: SubtitleTrack[]; expiresAt?: number; durationSec?: number; resumeSec?: number; skip?: { introEndSec?: number; outroStartSec?: number } }
export interface PlaybackParams { mediaId: string; episodeId: string; playlistId?: string; serverId?: string; reason: PlaybackReason; codecs: Codec[]; maxHeight?: number }
type Method<P, R> = (params: P, ctx: CallContext) => R | Promise<R>;
export interface FilterOption { id: string; label: string }
export interface Filter { id: string; label: string; options: FilterOption[]; multiple?: boolean }
export interface Category { id: string; label: string; children?: Category[]; filters?: Filter[]; sorts?: FilterOption[] }
export interface Section { id: string; title?: string; layout: "banner" | "row" | "grid" | "list"; imageAspect?: "portrait" | "landscape"; items: MediaItem[]; more?: boolean }
export interface Endpoint { id: string; label?: string; url: string }
export interface FormField { key: string; type: "text" | "password" | "email" | "number" | "otp" | "select" | "hidden" | "captcha"; label?: string; placeholder?: string; required?: boolean; value?: string; options?: { value: string; label: string }[]; image?: ImageRef }
export interface Form { title?: string; description?: string; fields: FormField[]; submitLabel?: string }
export interface UserProfile { id: string; name: string; avatar?: ImageRef; description?: string; stats?: Record<string, number> }
export interface AuthMethod { id: string; label: string; form?: Form }
export type AuthResult = { status: "ok"; user: UserProfile } | { status: "next"; form: Form };
export interface ArticleItem { id: string; title: string; summary?: string; cover?: ImageRef; tags?: string[]; author?: Author; stats?: Partial<Record<"likes" | "comments" | "favorites" | "hot", number>>; publishedAt?: number; mediaId?: string }
export interface ArticleDetail extends ArticleItem { contentHtml?: string; contentText?: string; streams?: Stream[]; interaction?: { liked?: boolean; favorited?: boolean } }
export type CommentTarget = { kind: "media"; mediaId: string; episodeId?: string } | { kind: "article"; articleId: string } | { kind: "replies"; commentId: string };
export interface Comment { id: string; author: { id?: string; name: string; avatar?: ImageRef }; text: string; createdAt?: number; likes?: number; liked?: boolean; replyCount?: number; replies?: Comment[]; pinned?: boolean; region?: string }
export type InteractTarget = Exclude<CommentTarget, { kind: "replies" }>;
export interface Channel { id: string; name: string; logo?: ImageRef; number?: number; groupId?: string; now?: string }
export interface Programme { title: string; startAt: number; endAt: number; description?: string }
export interface TransformParams { kind: "image" | "playlist" | "segment" | "key"; name: string; state?: Json; url: string; contentType?: string; data: Uint8Array }
export interface Plugin {
  media?: { getDetail: Method<{ id: string }, MediaDetail>; resolvePlayback: Method<PlaybackParams, PlaybackInfo>; listEpisodes?: Method<PageParams & { mediaId: string; playlistId: string }, Page<Episode>>; matchUrl?: Method<{ url: string }, { mediaId: string; episodeId?: string } | null> };
  resolver?: { resolve: Method<{ url: string; headers?: Record<string, string>; serverId?: string; reason: PlaybackReason; codecs: Codec[]; maxHeight?: number }, PlaybackInfo> };
  search?: { search: Method<PageParams & { query: string; filters?: Record<string, string[]> }, Page<MediaItem>>; getFilters?: Method<Record<string, never>, Filter[]>; suggest?: Method<{ query: string }, string[]>; trending?: Method<Record<string, never>, string[]> };
  home?: { getSections: Method<PageParams, Page<Section>>; listSection?: Method<PageParams & { sectionId: string }, Page<MediaItem>> };
  feed?: { list: Method<PageParams, Page<MediaItem>> };
  catalog?: { getCategories: Method<Record<string, never>, Category[]>; list: Method<PageParams & { categoryId: string; filters?: Record<string, string[]>; sort?: string }, Page<MediaItem>>; getFilters?: Method<{ categoryId: string }, { filters?: Filter[]; sorts?: FilterOption[] }> };
  live?: { getGroups: Method<Record<string, never>, { id: string; label: string }[]>; listChannels: Method<PageParams & { groupId?: string }, Page<Channel>>; resolve: Method<{ channelId: string; serverId?: string; codecs: Codec[]; maxHeight?: number; reason: PlaybackReason }, PlaybackInfo>; getProgrammes?: Method<{ channelId: string; from: number; to: number }, Programme[]> };
  articles?: { list: Method<PageParams, Page<ArticleItem>>; getDetail: Method<{ id: string }, ArticleDetail> };
  comments?: { list: Method<PageParams & { target: CommentTarget }, Page<Comment>>; send?: Method<{ target: CommentTarget; text: string }, Comment | void>; like?: Method<{ commentId: string; on: boolean }, void> };
  interact?: { like?: Method<{ target: InteractTarget; on: boolean }, void>; favorite?: Method<{ target: InteractTarget; on: boolean; folderId?: string }, void>; follow?: Method<{ authorId: string; on: boolean }, void> };
  auth?: { getMethods: Method<Record<string, never>, { methods: AuthMethod[]; registerUrl?: string }>; submit: Method<{ methodId: string; values: Record<string, string> }, AuthResult>; getProfile: Method<Record<string, never>, UserProfile | null>; logout: Method<Record<string, never>, void> };
  library?: { listMedia: Method<PageParams & { kind: "favorites" | "likes" | "history"; folderId?: string }, Page<MediaItem>>; listFolders?: Method<Record<string, never>, { id: string; label: string; count?: number }[]>; listFollowing?: Method<PageParams, Page<Author>> };
  progress?: { report: Method<{ mediaId: string; episodeId: string; positionSec: number; durationSec?: number; event: "start" | "heartbeat" | "pause" | "end" }, void> };
  updates?: { check: Method<{ items: { mediaId: string; episodeCount?: number; updatedAt?: number }[] }, { mediaId: string; episodeCount?: number; updatedAt?: number; latestLabel?: string }[]> };
  endpoints?: { list: Method<{ refresh?: boolean }, Endpoint[]> };
  ui?: { getActions: Method<Record<string, never>, { id: string; label: string; description?: string; form: Form }[]>; submit: Method<{ actionId: string; values: Record<string, string> }, { status: "ok"; message?: string } | { status: "next"; form: Form }> };
  transformResource?: Method<TransformParams, { data: Uint8Array; contentType?: string }>;
  dispose?: () => void;
}
export interface HostEnv { protocol: string; app: { name: string; version: string }; plugin: { id: string; version: string }; platform: "windows" | "macos" | "linux" | "android" | "ios"; locale: string; features: readonly string[]; media: { codecs: readonly Codec[]; formats: readonly MediaFormat[] } }
export interface HttpRequest { url: string; method?: "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE"; headers?: Record<string, string>; query?: Record<string, string | number | boolean | undefined>; body?: string | Uint8Array | { json: Json } | { form: Record<string, string> }; responseType?: "text" | "json" | "bytes"; charset?: string; timeoutMs?: number; redirect?: "follow" | "manual"; maxBytes?: number; cache?: { ttlSec: number; staleIfErrorSec?: number }; retry?: 0 | 1 | 2; credentials?: "include" | "omit"; signal?: AbortSignal }
export interface HttpResponse<T> { status: number; ok: boolean; url: string; headers: Record<string, string>; setCookies: string[]; data: T; fromCache: boolean }
export interface Cookie { name: string; value: string; domain?: string; path?: string; hostOnly?: boolean; expiresAt?: number; secure?: boolean; httpOnly?: boolean }
export interface HtmlNode { readonly tag: string; select(selector: string): HtmlNode[]; first(selector: string): HtmlNode | null; text(): string; ownText(): string; attr(name: string): string | null; attrs(): Record<string, string>; html(): string; outerHtml(): string; parent(): HtmlNode | null; children(): HtmlNode[] }
export type Hash = "md5" | "sha1" | "sha256" | "sha512";
export interface AesParams { mode: "cbc" | "ecb" | "ctr" | "gcm"; key: Uint8Array; iv?: Uint8Array; padding?: "pkcs7" | "none"; aad?: Uint8Array }
export interface RsaParams { publicKey: string; padding: "pkcs1" | "oaep-sha1" | "oaep-sha256" }
export interface HostApi {
  env: HostEnv; endpoint: { readonly current: Endpoint | null; url(path: string): string };
  settings: { get<T>(key: string): T | undefined; getAll(): Record<string, Json> };
  http: { fetch<T = string>(request: HttpRequest): Promise<HttpResponse<T>> };
  /** get/set operate on ordinary cookies. clear also ends the managed login for the URL origin. */
  cookies: { get(params: { url: string }): Promise<Cookie[]>; set(params: { url: string; cookie: Cookie }): Promise<void>; clear(params?: { url?: string }): Promise<void> };
  storage: { get<T extends TransferValue>(key: string): Promise<T | undefined>; set(key: string, value: TransferValue, options?: { ttlSec?: number }): Promise<void>; delete(key: string): Promise<void>; keys(prefix?: string): Promise<string[]>; clear(): Promise<void> };
  crypto: { digest(algorithm: Hash, data: string | Uint8Array): Uint8Array; hmac(algorithm: Hash, key: string | Uint8Array, data: string | Uint8Array): Uint8Array; aesEncrypt(params: AesParams, data: Uint8Array): Uint8Array; aesDecrypt(params: AesParams, data: Uint8Array): Uint8Array; rsaEncrypt(params: RsaParams, data: Uint8Array): Uint8Array; randomBytes(length: number): Uint8Array; randomUUID(): string };
  codec: { base64Encode(data: string | Uint8Array, options?: { urlSafe?: boolean; pad?: boolean }): string; base64Decode(text: string): Uint8Array; hexEncode(data: string | Uint8Array): string; hexDecode(text: string): Uint8Array; utf8Encode(text: string): Uint8Array; utf8Decode(data: Uint8Array): string; decodeText(data: Uint8Array, charset: string): string };
  html: { parse(html: string): HtmlNode }; log: Record<"debug" | "info" | "warn" | "error", (...args: unknown[]) => void>;
  /** Native host retains the first-party login session. Read account data with same-origin http.fetch. */
  webview?: { open(params: { url: string; title?: string; doneWhen: { urlPrefix?: string; cookieNames?: string[] }; timeoutSec?: number }): Promise<{ finalUrl: string }> };
  PluginError: new (code: ErrorCode, message?: string, options?: { retryAfterSec?: number; retryable?: boolean; details?: Json; cause?: unknown }) => Error & PluginFailure;
}
export type PluginFactory = (hg: HostApi) => Plugin | Promise<Plugin>;
