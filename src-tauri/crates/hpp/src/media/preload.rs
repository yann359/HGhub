//! Bounded, session-local media warmup. No second player and no playback events.
use super::*;
use axum::body::{to_bytes, Bytes};
use std::time::Instant;

const BUDGET: usize = 16 * 1024 * 1024;
const PREFIX: usize = 2 * 1024 * 1024;

#[derive(Clone)]
struct Entry {
    resource: usize,
    headers: HeaderMap,
    data: Bytes,
    start: u64,
    total: u64,
    at: Instant,
    policy: (u64, bool),
    request_range: Option<String>,
}
#[derive(Default)]
pub(super) struct Cache {
    entries: Mutex<Vec<Entry>>,
    pending: tokio::sync::Mutex<()>,
    cancel: Mutex<Option<CancellationToken>>,
}
impl Cache {
    pub(super) fn cancel(&self) {
        if let Some(token) = self.cancel.lock().unwrap().take() {
            token.cancel();
        }
    }
}

fn content_range(value: &str) -> Option<(u64, u64, u64)> {
    let (range, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let (start, end, total) = (start.parse().ok()?, end.parse().ok()?, total.parse().ok()?);
    (start <= end && end < total).then_some((start, end, total))
}
fn requested(headers: &HeaderMap, total: u64) -> Option<(u64, u64)> {
    if total == 0 {
        return None;
    }
    let Some(raw) = headers.get("range") else {
        return Some((0, total - 1));
    };
    let (start, end) = raw.to_str().ok()?.strip_prefix("bytes=")?.split_once('-')?;
    if start.is_empty() {
        let count: u64 = end.parse().ok()?;
        return (count > 0).then_some((total.saturating_sub(count), total - 1));
    }
    let start: u64 = start.parse().ok()?;
    let end = if end.is_empty() {
        total - 1
    } else {
        end.parse::<u64>().ok()?.min(total - 1)
    };
    (start <= end && start < total).then_some((start, end))
}
fn validator(headers: &HeaderMap) -> Option<&axum::http::HeaderValue> {
    headers
        .get("etag")
        .filter(|v| !v.as_bytes().starts_with(b"W/"))
        .or_else(|| headers.get("last-modified"))
}

pub(super) async fn cached(
    manager: &Arc<PluginManager>,
    session: &Arc<Session>,
    id: &str,
    resource: &Resource,
    method: &Method,
    incoming: &HeaderMap,
) -> Result<Option<Response<Body>>> {
    if *method != Method::GET && *method != Method::HEAD {
        return Ok(None);
    }
    // The warmup and first foreground read share completed bytes. Cancellation
    // releases this lock; foreground never waits for another episode's warmup.
    let _pending = tokio::select! {
        _ = session.cancel.cancelled() => return Err(PluginError::cancelled()),
        lock = session.cache.pending.lock() => lock,
    };
    let index = session.register(resource.clone())?;
    let policy = (
        crate::proxy::generation(),
        session.providers[&resource.provider].0.strict(),
    );
    let entries = {
        let mut entries = session.cache.entries.lock().unwrap();
        entries.retain(|e| e.at.elapsed() < Duration::from_secs(120) && e.policy == policy);
        entries.clone()
    };
    drop(_pending);
    for entry in entries.iter().filter(|e| e.resource == index) {
        let Some((start, end)) = requested(incoming, entry.total) else {
            continue;
        };
        if incoming
            .get("if-range")
            .is_some_and(|v| validator(&entry.headers) != Some(v))
        {
            continue;
        }
        let cached_end = entry.start + entry.data.len() as u64;
        if start < entry.start || start >= cached_end {
            continue;
        }
        let prefix_end = (end + 1).min(cached_end);
        let prefix = entry
            .data
            .slice((start - entry.start) as usize..(prefix_end - entry.start) as usize);
        let body = if *method == Method::HEAD {
            Body::empty()
        } else if prefix_end > end {
            Body::from(prefix)
        } else {
            // Splice only a version-checked continuation, never join changed files.
            let Some(version) = validator(&entry.headers) else {
                continue;
            };
            let mut headers = incoming.clone();
            headers.insert(
                "range",
                format!("bytes={prefix_end}-{end}").parse().unwrap(),
            );
            headers.insert("if-range", version.clone());
            let tail = proxy_upstream(
                manager,
                session,
                id,
                resource.clone(),
                Method::GET,
                headers,
                MAX_RESOURCE,
            )
            .await?;
            if tail.status() != StatusCode::PARTIAL_CONTENT
                || tail
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(content_range)
                    != Some((prefix_end, end, entry.total))
                || validator(tail.headers()) != Some(version)
            {
                continue;
            }
            Body::from_stream(
                futures_util::stream::once(async move { Ok::<_, axum::Error>(prefix) })
                    .chain(tail.into_body().into_data_stream()),
            )
        };
        let partial = incoming.contains_key("range");
        let mut out = Response::builder().status(if partial { 206 } else { 200 });
        for name in ["content-type", "etag", "last-modified"] {
            if let Some(value) = entry.headers.get(name) {
                out = out.header(name, value);
            }
        }
        out = out
            .header("content-length", end - start + 1)
            .header("accept-ranges", "bytes");
        if partial {
            out = out.header(
                "content-range",
                format!("bytes {start}-{end}/{}", entry.total),
            );
        }
        return Ok(Some(out.body(body).unwrap()));
    }
    Ok(None)
}

impl PluginManager {
    pub async fn preload_media(self: &Arc<Self>, id: &str, position: f64) -> Result<Value> {
        let session = self
            .sessions
            .sessions
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| PluginError::new("NOT_FOUND", "媒体会话已释放"))?;
        // One low-priority operation globally; never queue a stale episode.
        let Ok(_gate) = self.sessions.preload_gate.try_lock() else {
            return Ok(json!({"bytes":0}));
        };
        if session.playback.load(Ordering::Acquire) {
            return Ok(json!({"bytes":0}));
        }
        for (owner, (_, revision)) in &session.providers {
            if self.revision(owner).await? != *revision {
                return Err(PluginError::cancelled());
            }
        }
        if session.expires_at.is_some_and(|t| t <= now() + 5000) {
            return Err(PluginError::cancelled());
        }
        let cancel = session.cancel.child_token();
        let _transfer = {
            // Register under the same lock as foreground cancellation. A newly
            // arriving warmup cannot slip past a queued or streaming play request.
            let _sessions = self.sessions.sessions.lock().unwrap();
            if self.sessions.foreground.load(Ordering::Acquire) > 0 {
                return Ok(json!({"bytes":0}));
            }
            let Ok(permit) = MEDIA_TRANSFERS.clone().try_acquire_owned() else {
                return Ok(json!({"bytes":0}));
            };
            session.cache.cancel();
            *session.cache.cancel.lock().unwrap() = Some(cancel.clone());
            permit
        };
        let work = async {
            let root = session
                .resources
                .lock()
                .unwrap()
                .first()
                .cloned()
                .ok_or_else(PluginError::cancelled)?;
            if root.kind == "media" {
                self.warm_resource(
                    &session,
                    id,
                    0,
                    Some(format!("bytes=0-{}", PREFIX - 1)),
                    &cancel,
                )
                .await?;
                // MP4 metadata can live at the end of the file.
                self.warm_resource(&session, id, 0, Some("bytes=-524288".into()), &cancel)
                    .await?;
            } else if root.kind == "playlist" {
                let mut queue = std::collections::VecDeque::from([0usize]);
                let mut visited = std::collections::HashSet::new();
                while let Some(index) = queue.pop_front() {
                    if !visited.insert(index) || visited.len() > 5 {
                        continue;
                    }
                    let bytes = self
                        .warm_resource(&session, id, index, None, &cancel)
                        .await?;
                    let text = std::str::from_utf8(&bytes)
                        .map_err(|_| PluginError::invalid("HLS 清单编码无效"))?;
                    let (playlists, resources) = plan(text, position);
                    queue.extend(playlists);
                    for (index, range) in resources {
                        self.warm_resource(&session, id, index, range, &cancel)
                            .await?;
                    }
                }
            }
            Ok::<_, PluginError>(())
        };
        let result = tokio::select! {
            _ = cancel.cancelled() => Err(PluginError::cancelled()),
            value = tokio::time::timeout(Duration::from_secs(15), work) => value.unwrap_or_else(|_| Err(PluginError::timeout())),
        };
        cancel.cancel();
        result?;
        let bytes: usize = session
            .cache
            .entries
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.data.len())
            .sum();
        Ok(json!({"bytes":bytes}))
    }

    async fn warm_resource(
        self: &Arc<Self>,
        session: &Arc<Session>,
        id: &str,
        index: usize,
        range: Option<String>,
        cancel: &CancellationToken,
    ) -> Result<Bytes> {
        if cancel.is_cancelled() || session.playback.load(Ordering::Acquire) {
            return Err(PluginError::cancelled());
        }
        let _pending = tokio::select! { _ = cancel.cancelled() => return Err(PluginError::cancelled()), value = session.cache.pending.lock() => value };
        let r = session
            .resources
            .lock()
            .unwrap()
            .get(index)
            .cloned()
            .ok_or_else(PluginError::cancelled)?;
        let policy = (
            crate::proxy::generation(),
            session.providers[&r.provider].0.strict(),
        );
        if range.is_some() && r.transforms.get(&r.kind).is_some_and(|v| !v.is_null()) {
            return Err(PluginError::new(
                "UNSUPPORTED",
                "变换后的渐进流不支持有限预取",
            ));
        }
        let used = {
            let mut entries = session.cache.entries.lock().unwrap();
            entries.retain(|e| e.at.elapsed() < Duration::from_secs(120) && e.policy == policy);
            if let Some(entry) = entries
                .iter()
                .find(|e| e.resource == index && e.request_range == range)
            {
                return Ok(entry.data.clone());
            }
            entries.iter().map(|e| e.data.len()).sum::<usize>()
        };
        let limit = BUDGET.saturating_sub(used).min(MAX_RESOURCE);
        if limit == 0 {
            return Err(PluginError::cancelled());
        }
        let mut headers = HeaderMap::new();
        if let Some(range) = &range {
            headers.insert(
                "range",
                range
                    .parse()
                    .map_err(|_| PluginError::invalid("无效预取范围"))?,
            );
        }
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(PluginError::cancelled()),
            value = proxy_upstream(self, session, id, r.clone(), Method::GET, headers, limit) => value?,
        };
        let status = response.status();
        if !status.is_success() {
            return Err(PluginError::new("UPSTREAM_ERROR", "预取资源不可用"));
        }
        // A server ignoring a progressive Range must never download the whole movie.
        if range.is_some() && status != StatusCode::PARTIAL_CONTENT {
            return Err(PluginError::new("UNSUPPORTED", "来源不支持有限预取"));
        }
        let headers = response.headers().clone();
        if headers
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok())
            .is_some_and(|n| n > limit)
        {
            return Err(PluginError::new("UNSUPPORTED", "预取资源超过预算"));
        }
        let data = tokio::select! {
            _ = cancel.cancelled() => return Err(PluginError::cancelled()),
            bytes = to_bytes(response.into_body(), limit) => bytes.map_err(|_| PluginError::new("UPSTREAM_ERROR", "预取资源超过预算或读取失败"))?,
        };
        let (start, total) = if status == StatusCode::PARTIAL_CONTENT {
            let (start, end, total) = headers
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .and_then(content_range)
                .ok_or_else(|| PluginError::invalid("预取范围响应无效"))?;
            if end - start + 1 != data.len() as u64 {
                return Err(PluginError::invalid("预取范围长度无效"));
            }
            (start, total)
        } else {
            (0, data.len() as u64)
        };
        let dynamic = r.kind == "playlist"
            && std::str::from_utf8(&data)
                .is_ok_and(|text| text.contains("#EXTINF:") && !text.contains("#EXT-X-ENDLIST"));
        if !data.is_empty()
            && !dynamic
            && policy
                == (
                    crate::proxy::generation(),
                    session.providers[&r.provider].0.strict(),
                )
        {
            // Bound all retained warm data, including already adopted sessions.
            let all = self.sessions.sessions.lock().unwrap();
            let mut retained: usize = all
                .values()
                .map(|s| {
                    s.cache
                        .entries
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|e| e.data.len())
                        .sum::<usize>()
                })
                .sum();
            for (other_id, other) in all.iter() {
                if retained + data.len() <= 64 * 1024 * 1024 {
                    break;
                }
                if other_id == id {
                    continue;
                }
                let mut old = other.cache.entries.lock().unwrap();
                retained = retained.saturating_sub(old.iter().map(|e| e.data.len()).sum());
                old.clear();
            }
            drop(all);
            session.cache.entries.lock().unwrap().push(Entry {
                resource: index,
                headers,
                data: data.clone(),
                start,
                total,
                at: Instant::now(),
                policy,
                request_range: range,
            });
        }
        Ok(data)
    }
}

// Parse only rewritten local references; no upstream URL or credential crosses IPC.
// Unsupported byte-range playlists are left to normal playback rather than warmed incorrectly.
fn plan(text: &str, position: f64) -> (Vec<usize>, Vec<(usize, Option<String>)>) {
    let id = |raw: &str| raw.trim().split('?').next()?.rsplit('/').next()?.parse::<usize>().ok();
    let uri = regex::Regex::new(r#"URI="([^"]*)""#).unwrap();
    let attr = |line: &str| uri.captures(line).and_then(|c| id(&c[1]));
    let mut playlists = vec![];
    let mut audio = None;
    let mut variant = false;
    let mut resources = vec![];
    let (mut time, mut duration, mut count) = (0.0, 0.0, 0);
    let (mut key, mut map) = (None, None);
    let mut chosen_audio_group: Option<String> = None;
    let group = regex::Regex::new(r#"(?:AUDIO|GROUP-ID)="([^"]*)""#).unwrap();
    for line in text.lines().map(str::trim) {
        if line.starts_with("#EXT-X-STREAM-INF:") && playlists.is_empty() {
            variant = true;
            chosen_audio_group = group.captures(line).map(|c| c[1].to_string());
        } else if !line.starts_with('#') && !line.is_empty() && variant {
            if let Some(index) = id(line) {
                playlists.push(index);
            }
            variant = false;
        }
    }
    if !playlists.is_empty() {
        for line in text
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("#EXT-X-MEDIA:") && l.contains("TYPE=AUDIO,"))
        {
            if group.captures(line).map(|c| c[1].to_string()) != chosen_audio_group {
                continue;
            }
            if audio.is_none() || line.contains("DEFAULT=YES") {
                audio = attr(line);
            }
            if line.contains("DEFAULT=YES") {
                break;
            }
        }
        if let Some(audio) = audio {
            playlists.push(audio);
        }
        return (playlists, resources);
    }
    if !text.contains("#EXT-X-ENDLIST") || text.contains("BYTERANGE") {
        return (vec![], vec![]);
    }
    for line in text.lines().map(str::trim) {
        if line.starts_with("#EXT-X-KEY:") {
            key = attr(line);
        } else if line.starts_with("#EXT-X-MAP:") {
            map = attr(line);
        } else if let Some(value) = line.strip_prefix("#EXTINF:") {
            duration = value
                .split(',')
                .next()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
        } else if !line.starts_with('#') && !line.is_empty() {
            if time + duration > position.max(0.0) && count < 2 {
                for index in [key, map, id(line)].into_iter().flatten() {
                    if !resources.iter().any(|(old, _)| *old == index) {
                        resources.push((index, None));
                    }
                }
                count += 1;
            }
            time += duration;
        }
    }
    (vec![], resources)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn warm_plan_uses_resume_dependencies_and_selected_audio() {
        let (_, parts) = plan("#EXTM3U\n#EXT-X-KEY:URI=\"http://local/1\"\n#EXT-X-MAP:URI=\"http://local/2\"\n#EXTINF:10,\nhttp://local/3\n#EXTINF:10,\nhttp://local/4\n#EXT-X-ENDLIST", 12.0);
        assert_eq!(parts, vec![(1, None), (2, None), (4, None)]);
        let (lists, _) = plan("#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",DEFAULT=YES,URI=\"http://local/2\"\n#EXT-X-STREAM-INF:BANDWIDTH=10,AUDIO=\"a\"\nhttp://local/1\n#EXT-X-STREAM-INF:BANDWIDTH=20\nhttp://local/3", 0.0);
        assert_eq!(lists, vec![1, 2]);
        assert!(plan("#EXTM3U\n#EXTINF:10,\nhttp://local/1", 0.0)
            .1
            .is_empty());
    }
}
