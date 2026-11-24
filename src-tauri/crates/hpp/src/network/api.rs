use super::*;
use reqwest::StatusCode;
use tokio::sync::{watch, OwnedSemaphorePermit, Semaphore};

#[derive(Clone, Copy)]
pub enum Queue {
    Api,
    Media,
}

struct Permits {
    _plugin: OwnedSemaphorePermit,
    _host: OwnedSemaphorePermit,
}

pub(super) struct Outgoing {
    target: Url,
    method: Method,
    headers: HeaderMap,
    body: Option<Vec<u8>>,
    cookies: bool,
    generation: Option<String>,
}

pub(super) struct BufferedResponse {
    pub status: StatusCode,
    pub url: Url,
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
    pub redacted: bool,
}

pub(super) struct Flight {
    pub(super) result: watch::Sender<Option<Result<Arc<BufferedResponse>>>>,
}

impl Network {
    pub(super) fn outgoing(
        &self,
        target: Url,
        method: Method,
        mut headers: HeaderMap,
        body: Option<Vec<u8>>,
        cookies: bool,
    ) -> Result<Outgoing> {
        let (generation, cookie) =
            self.request_cookies(&target, cookies && !headers.contains_key("cookie"));
        if !cookie.is_empty() {
            headers.insert(
                "cookie",
                HeaderValue::from_str(&cookie).map_err(|_| PluginError::invalid("Cookie 无效"))?,
            );
        }
        headers
            .entry("user-agent")
            .or_insert(HeaderValue::from_static("HGhub/HPP-dev"));
        Ok(Outgoing {
            target,
            method,
            headers,
            body,
            cookies,
            generation,
        })
    }

    fn check_cooldown(&self, host: &str) -> Result<()> {
        if let Some(until) = self.cooldown.lock().unwrap().get(host).copied() {
            let remaining = until.saturating_duration_since(Instant::now());
            if !remaining.is_zero() {
                let mut e = PluginError::new("RATE_LIMITED", "该主机正在冷却");
                e.retry_after_sec = Some(remaining.as_secs().max(1));
                return Err(e);
            }
        }
        Ok(())
    }

    async fn permits(&self, host: &str, cancel: &CancellationToken) -> Result<Permits> {
        let gate = {
            let mut hosts = self.hosts.lock().unwrap();
            hosts.retain(|_, gate| gate.strong_count() > 0);
            if let Some(gate) = hosts.get(host).and_then(Weak::upgrade) {
                gate
            } else {
                let gate = Arc::new(Semaphore::new(4));
                hosts.insert(host.to_string(), Arc::downgrade(&gate));
                gate
            }
        };
        // A saturated host must not occupy all plugin slots while waiting.
        let host = tokio::select! {
            _ = cancel.cancelled() => return Err(PluginError::cancelled()),
            value = gate.acquire_owned() => value.map_err(|_| PluginError::internal("网络队列已关闭"))?,
        };
        let plugin = tokio::select! {
            _ = cancel.cancelled() => return Err(PluginError::cancelled()),
            value = self.gate.clone().acquire_owned() => value.map_err(|_| PluginError::internal("网络队列已关闭"))?,
        };
        Ok(Permits {
            _plugin: plugin,
            _host: host,
        })
    }

    pub(super) async fn send(
        &self,
        outgoing: Outgoing,
        queue: Queue,
        cancel: &CancellationToken,
    ) -> Result<Response> {
        if cancel.is_cancelled() {
            return Err(PluginError::cancelled());
        }
        let host = outgoing.target.host_str().unwrap();
        self.check_cooldown(host)?;
        let permits = if matches!(queue, Queue::Api) {
            let permits = self.permits(host, cancel).await?;
            self.rate_limit(cancel).await?;
            Some(Arc::new(permits))
        } else {
            None
        };
        self.check_cooldown(host)?;
        let client = self.client(&outgoing.target, cancel).await?;
        let mut request = client
            .request(outgoing.method, outgoing.target.clone())
            .headers(outgoing.headers);
        if let Some(body) = outgoing.body {
            request = request.body(body);
        }
        let mut response = tokio::select! {
            _ = cancel.cancelled() => return Err(PluginError::cancelled()),
            value = request.send() => value.map_err(|e| PluginError::new("NETWORK", crate::proxy::connection_error(&e)))?,
        };
        if self.receive_cookies(
            &outgoing.target,
            response.headers_mut(),
            outgoing.generation.as_deref(),
            outgoing.cookies,
        )? {
            response.extensions_mut().insert(RedactedSetCookie);
        }
        if matches!(response.status().as_u16(), 429 | 503) {
            if let Some(sec) = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
            {
                self.cooldown.lock().unwrap().insert(
                    host.to_string(),
                    Instant::now() + Duration::from_secs(sec.min(600)),
                );
            }
        }
        // API callers retain Response through chunk reads. Media streams use their
        // own permit, retained by the loopback response body.
        if let Some(permits) = permits {
            response.extensions_mut().insert(permits);
        }
        Ok(response)
    }

    pub(super) fn redirect(
        &self,
        target: &Url,
        status: StatusCode,
        headers: &HeaderMap,
        follow: bool,
        hop: usize,
    ) -> Result<Option<Url>> {
        if !follow || !matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
            return Ok(None);
        }
        if hop == 5 {
            return Err(PluginError::new("NETWORK", "重定向次数超过上限"));
        }
        let location = headers
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| PluginError::new("NETWORK", "重定向缺少 Location"))?;
        let next = target
            .join(location)
            .map_err(|_| PluginError::new("NETWORK", "重定向地址无效"))?;
        if target.scheme() == "https" && next.scheme() != "https" {
            return Err(PluginError::new("NETWORK", "禁止 HTTPS 降级"));
        }
        self.target(next.as_str()).map(Some)
    }

    pub(super) async fn buffered(
        self: &Arc<Self>,
        mut target: Url,
        method: Method,
        mut headers: HeaderMap,
        body: Option<Vec<u8>>,
        cookies: bool,
        follow: bool,
        max: usize,
        cancel: &CancellationToken,
    ) -> Result<Arc<BufferedResponse>> {
        let initial = target.origin();
        for hop in 0..=5 {
            target.set_fragment(None);
            if target.origin() != initial {
                headers.remove("authorization");
                headers.remove("cookie");
            }
            let outgoing = self.outgoing(
                target.clone(),
                method.clone(),
                headers.clone(),
                body.clone(),
                cookies,
            )?;
            let response = self.shared_hop(outgoing, follow, max, cancel).await?;
            if let Some(next) =
                self.redirect(&target, response.status, &response.headers, follow, hop)?
            {
                target = next;
            } else {
                return Ok(response);
            }
        }
        unreachable!("redirect rejects a sixth hop")
    }

    async fn read_hop(
        &self,
        outgoing: Outgoing,
        follow: bool,
        max: usize,
        cancel: &CancellationToken,
    ) -> Result<Arc<BufferedResponse>> {
        let mut response = self.send(outgoing, Queue::Api, cancel).await?;
        let mut bytes = Vec::new();
        if !follow || !matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
            while let Some(chunk) = tokio::select! {
                _ = cancel.cancelled() => return Err(PluginError::cancelled()),
                value = response.chunk() => value.map_err(|_| PluginError::new("NETWORK", "响应读取失败"))?,
            } {
                if bytes.len() + chunk.len() > max {
                    return Err(PluginError::new("UPSTREAM_ERROR", "响应体超过 maxBytes"));
                }
                bytes.extend_from_slice(&chunk);
            }
        }
        Ok(Arc::new(BufferedResponse {
            status: response.status(),
            url: response.url().clone(),
            headers: response.headers().clone(),
            bytes,
            redacted: response.extensions().get::<RedactedSetCookie>().is_some(),
        }))
    }

    async fn shared_hop(
        self: &Arc<Self>,
        outgoing: Outgoing,
        follow: bool,
        max: usize,
        cancel: &CancellationToken,
    ) -> Result<Arc<BufferedResponse>> {
        self.check_cooldown(outgoing.target.host_str().unwrap())?;
        if outgoing.method != Method::GET
            || outgoing.headers.contains_key("authorization")
            || outgoing.headers.contains_key("cookie")
        {
            return self.read_hop(outgoing, follow, max, cancel).await;
        }
        let key = json!([
            crate::proxy::generation(),
            self.strict(),
            outgoing.target.as_str(),
            outgoing
                .headers
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_bytes()))
                .collect::<std::collections::BTreeMap<_, _>>(),
            outgoing.cookies,
            outgoing.generation,
            follow,
            max,
        ])
        .to_string();
        let mut receiver = {
            let mut pending = self.inflight.lock().unwrap();
            if let Some(flight) = pending.get(&key).filter(|f| !f.result.is_closed()) {
                flight.result.subscribe()
            } else {
                let (result, receiver) = watch::channel(None);
                let flight = Arc::new(Flight { result });
                pending.insert(key.clone(), flight.clone());
                let network = self.clone();
                tokio::spawn(async move {
                    let cancel = CancellationToken::new();
                    let work = network.read_hop(outgoing, follow, max, &cancel);
                    tokio::pin!(work);
                    let result = loop {
                        tokio::select! {
                            biased;
                            _ = flight.result.closed() => {
                                let mut pending = network.inflight.lock().unwrap();
                                if flight.result.is_closed() {
                                    if pending.get(&key).is_some_and(|current| Arc::ptr_eq(current, &flight)) {
                                        pending.remove(&key);
                                    }
                                    break Err(PluginError::cancelled());
                                }
                            },
                            value = &mut work => break value,
                        }
                    };
                    cancel.cancel();
                    let mut pending = network.inflight.lock().unwrap();
                    if pending
                        .get(&key)
                        .is_some_and(|current| Arc::ptr_eq(current, &flight))
                    {
                        pending.remove(&key);
                    }
                    flight.result.send_replace(Some(result));
                });
                receiver
            }
        };
        loop {
            if cancel.is_cancelled() {
                return Err(PluginError::cancelled());
            }
            if let Some(result) = receiver.borrow().clone() {
                return result;
            }
            tokio::select! {
                _ = cancel.cancelled() => return Err(PluginError::cancelled()),
                changed = receiver.changed() => changed.map_err(|_| PluginError::internal("网络合并请求缺少结果"))?,
            }
        }
    }
}
