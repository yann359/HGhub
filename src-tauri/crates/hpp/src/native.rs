use crate::{wire::Transfer, PluginError, Result};
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, KeyIvInit, StreamCipher};
use base64::Engine;
use rsa::{pkcs1::DecodeRsaPublicKey, pkcs8::DecodePublicKey, PublicKey};
use serde_json::{json, Value};
use sha2::Digest;
use std::collections::HashMap;
mod logs;
pub(crate) use logs::validation_log;
pub use logs::Logs;
pub struct Native {
    pub logs: Logs,
    docs: HashMap<u64, (String, dom_query::Document)>,
    nodes: HashMap<u64, (u64, dom_query::NodeId)>,
    next: u64,
}
impl Native {
    pub fn new(logs: Logs) -> Self {
        Self {
            logs,
            docs: HashMap::new(),
            nodes: HashMap::new(),
            next: 0,
        }
    }
    pub fn clear(&mut self, scope: &str) {
        self.docs.retain(|_, (s, _)| s != scope);
        self.nodes.retain(|_, (d, _)| self.docs.contains_key(d));
    }
    pub fn sync(&mut self, op: &str, p: Transfer, scope: &str) -> Result<Transfer> {
        let v = p.json();
        let string = |k: &str| {
            v[k].as_str()
                .map(String::from)
                .ok_or_else(|| PluginError::invalid(format!("缺少文本参数 {k}")))
        };
        let bytes = |k: &str| {
            p.field(k)
                .ok_or_else(|| PluginError::invalid(format!("缺少字节参数 {k}")))?
                .bytes()
        };
        let json_result = match op {
            "env" => {
                json!({"platform":if cfg!(target_os="windows"){"windows"}else if cfg!(target_os="macos"){"macos"}else if cfg!(target_os="android"){"android"}else if cfg!(target_os="ios"){"ios"}else{"linux"}})
            }
            "log" => {
                self.logs.lock().unwrap().push(v["level"].as_str().unwrap(), v["text"].as_str().unwrap(), scope, None);
                Value::Null
            }
            "codec.utf8Encode" => return Ok(Transfer::Bytes(string("text")?.into_bytes())),
            "codec.decodeText" => {
                let b = bytes("data")?;
                let label = string("charset")?;
                let enc = encoding_rs::Encoding::for_label(label.as_bytes())
                    .ok_or_else(|| PluginError::invalid("未知字符集"))?;
                json!(enc.decode(&b).0)
            }
            "codec.base64Encode" => {
                let b = bytes("data")?;
                let enc = match (
                    v["urlSafe"].as_bool().unwrap_or(false),
                    v["pad"].as_bool().unwrap_or(true),
                ) {
                    (true, true) => &base64::engine::general_purpose::URL_SAFE,
                    (true, false) => &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                    (false, true) => &base64::engine::general_purpose::STANDARD,
                    (false, false) => &base64::engine::general_purpose::STANDARD_NO_PAD,
                };
                json!(enc.encode(b))
            }
            "codec.base64Decode" => {
                let s = string("text")?
                    .replace(['\n', '\r', ' '], "")
                    .replace('-', "+")
                    .replace('_', "/");
                let config = base64::engine::general_purpose::GeneralPurposeConfig::new()
                    .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent);
                return Ok(Transfer::Bytes(
                    base64::engine::general_purpose::GeneralPurpose::new(
                        &base64::alphabet::STANDARD,
                        config,
                    )
                    .decode(s)
                    .map_err(|_| PluginError::invalid("Base64 格式无效"))?,
                ));
            }
            "codec.hexEncode" => json!(bytes("data")?
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()),
            "codec.hexDecode" => {
                let s = string("text")?;
                if s.len() % 2 != 0 {
                    return Err(PluginError::invalid("十六进制长度无效"));
                }
                return Ok(Transfer::Bytes(
                    s.as_bytes()
                        .chunks(2)
                        .map(|c| {
                            std::str::from_utf8(c)
                                .ok()
                                .and_then(|s| u8::from_str_radix(s, 16).ok())
                                .ok_or_else(|| PluginError::invalid("十六进制格式无效"))
                        })
                        .collect::<Result<Vec<_>>>()?,
                ));
            }
            "crypto.randomUUID" => json!(uuid::Uuid::new_v4().to_string()),
            "crypto.randomBytes" => {
                use rand::RngCore;
                let n = v["length"]
                    .as_u64()
                    .filter(|n| *n <= 65536)
                    .ok_or_else(|| PluginError::invalid("随机字节长度无效"))?;
                let mut b = vec![0; n as usize];
                rand::rngs::OsRng.fill_bytes(&mut b);
                return Ok(Transfer::Bytes(b));
            }
            "crypto.digest" => {
                let b = bytes("data")?;
                let out = match string("algorithm")?.as_str() {
                    "md5" => md5::Md5::digest(&b).to_vec(),
                    "sha1" => sha1::Sha1::digest(&b).to_vec(),
                    "sha256" => sha2::Sha256::digest(&b).to_vec(),
                    "sha512" => sha2::Sha512::digest(&b).to_vec(),
                    _ => return Err(PluginError::invalid("未知摘要算法")),
                };
                return Ok(Transfer::Bytes(out));
            }
            "crypto.hmac" => {
                use hmac::Mac;
                let key = bytes("key")?;
                let data = bytes("data")?;
                macro_rules! mac {
                    ($t:ty) => {{
                        let mut m = <hmac::Hmac<$t> as Mac>::new_from_slice(&key)
                            .map_err(|_| PluginError::invalid("HMAC key 无效"))?;
                        m.update(&data);
                        m.finalize().into_bytes().to_vec()
                    }};
                }
                return Ok(Transfer::Bytes(match string("algorithm")?.as_str() {
                    "md5" => mac!(md5::Md5),
                    "sha1" => mac!(sha1::Sha1),
                    "sha256" => mac!(sha2::Sha256),
                    "sha512" => mac!(sha2::Sha512),
                    _ => return Err(PluginError::invalid("未知 HMAC 算法")),
                }));
            }
            "crypto.rsaEncrypt" => {
                let padding = string("padding")?;
                if !matches!(padding.as_str(), "pkcs1" | "oaep-sha1" | "oaep-sha256") {
                    return Err(PluginError::invalid("未知 RSA 填充方式"));
                }
                let data = match p.field("data") {
                    Some(Transfer::Bytes(data)) => data,
                    _ => return Err(PluginError::invalid("RSA data 必须是 Uint8Array")),
                };
                let pem = string("publicKey")?;
                // Both decoders validate the modulus (1024–8192 bits) and exponent.
                let key = PublicKey::from_public_key_pem(&pem)
                    .or_else(|_| PublicKey::from_pkcs1_pem(&pem))
                    .map_err(|_| {
                        PluginError::invalid("RSA 公钥无效：需要 1024–8192 位 SPKI 或 PKCS#1 PEM")
                    })?;
                // The library uses the operating system RNG; OAEP uses an empty label
                // and the selected digest for both OAEP and MGF1.
                let encrypted = match padding.as_str() {
                    "pkcs1" => key.pkcs1v15_encrypt(data),
                    "oaep-sha1" => key.oaep_encrypt::<rsa_sha1::Sha1>(data),
                    "oaep-sha256" => key.oaep_encrypt::<rsa::sha2::Sha256>(data),
                    _ => unreachable!(),
                }
                .map_err(|error| match error {
                    rsa::Error::Rng | rsa::Error::Internal => PluginError::internal("RSA 加密失败"),
                    _ => PluginError::invalid("RSA 明文过长或参数无效"),
                })?;
                return Ok(Transfer::Bytes(encrypted));
            }
            "crypto.aesEncrypt" | "crypto.aesDecrypt" => {
                return Ok(Transfer::Bytes(aes_transform(
                    &v,
                    bytes("key")?,
                    p.field("iv")
                        .map(Transfer::bytes)
                        .transpose()?
                        .unwrap_or_default(),
                    bytes("data")?,
                    p.field("aad")
                        .map(Transfer::bytes)
                        .transpose()?
                        .unwrap_or_default(),
                    op.ends_with("Encrypt"),
                )?))
            }
            "url.parse" | "url.set" => {
                let mut u = if op == "url.parse" {
                    if let Some(base) = v["base"].as_str() {
                        url::Url::parse(base)
                            .and_then(|b| b.join(v["input"].as_str().unwrap_or("")))
                    } else {
                        url::Url::parse(v["input"].as_str().unwrap_or(""))
                    }
                } else {
                    url::Url::parse(v["url"].as_str().unwrap_or(""))
                }
                .map_err(|_| PluginError::invalid("URL 无效"))?;
                if op == "url.set" {
                    let val = string("value")?;
                    match string("key")?.as_str() {
                        "href" => {
                            u = url::Url::parse(&val)
                                .map_err(|_| PluginError::invalid("URL 无效"))?
                        }
                        "pathname" => u.set_path(&val),
                        "search" => u.set_query(if val.is_empty() {
                            None
                        } else {
                            Some(val.trim_start_matches('?'))
                        }),
                        "hash" => u.set_fragment(if val.is_empty() {
                            None
                        } else {
                            Some(val.trim_start_matches('#'))
                        }),
                        "protocol" => {
                            u.set_scheme(val.trim_end_matches(':'))
                                .map_err(|_| PluginError::invalid("scheme 无效"))?;
                        }
                        "hostname" => u
                            .set_host(Some(&val))
                            .map_err(|_| PluginError::invalid("host 无效"))?,
                        "host" => {
                            let other = url::Url::parse(&format!("{}://{val}", u.scheme()))
                                .map_err(|_| PluginError::invalid("host 无效"))?;
                            u.set_host(other.host_str())
                                .map_err(|_| PluginError::invalid("host 无效"))?;
                            u.set_port(other.port())
                                .map_err(|_| PluginError::invalid("port 无效"))?;
                        }
                        "port" => {
                            u.set_port(if val.is_empty() {
                                None
                            } else {
                                Some(val.parse().map_err(|_| PluginError::invalid("port 无效"))?)
                            })
                            .map_err(|_| PluginError::invalid("port 无效"))?;
                        }
                        "username" => {
                            u.set_username(&val)
                                .map_err(|_| PluginError::invalid("username 无效"))?;
                        }
                        "password" => {
                            u.set_password(Some(&val))
                                .map_err(|_| PluginError::invalid("password 无效"))?;
                        }
                        _ => return Err(PluginError::invalid("URL 属性无效")),
                    }
                }
                json!({"href":u.as_str(),"origin":u.origin().ascii_serialization(),"protocol":format!("{}:",u.scheme()),"username":u.username(),"password":u.password().unwrap_or(""),"hostname":u.host_str().unwrap_or(""),"host":format!("{}{}",u.host_str().unwrap_or(""),u.port().map(|p|format!(":{p}")).unwrap_or_default()),"port":u.port().map(|p|p.to_string()).unwrap_or_default(),"pathname":u.path(),"search":u.query().map(|s|format!("?{s}")).unwrap_or_default(),"hash":u.fragment().map(|s|format!("#{s}")).unwrap_or_default()})
            }
            "url.queryParse" => json!(url::form_urlencoded::parse(string("text")?.as_bytes())
                .into_owned()
                .collect::<Vec<_>>()),
            "url.queryEncode" => {
                let mut s = url::form_urlencoded::Serializer::new(String::new());
                for pair in v["pairs"]
                    .as_array()
                    .ok_or_else(|| PluginError::invalid("查询参数无效"))?
                {
                    s.append_pair(
                        pair[0].as_str().unwrap_or(""),
                        pair[1].as_str().unwrap_or(""),
                    );
                }
                json!(s.finish())
            }
            "html.parse" => {
                if scope.is_empty() {
                    return Err(PluginError::new("UNSUPPORTED", "HTML 解析需要活动调用"));
                }
                let html = string("html")?;
                if html.len() > 8 * 1024 * 1024 || self.docs.len() >= 16 {
                    return Err(PluginError::invalid("HTML 资源预算超限"));
                }
                self.next += 1;
                let doc = self.next;
                let d = dom_query::Document::from(html);
                let root = d.root().id;
                self.docs.insert(doc, (scope.into(), d));
                self.next += 1;
                let handle = self.next;
                self.nodes.insert(handle, (doc, root));
                json!(handle)
            }
            "html.node" => {
                let handle = v["handle"]
                    .as_u64()
                    .ok_or_else(|| PluginError::invalid("节点句柄无效"))?;
                let (doc, node) = *self
                    .nodes
                    .get(&handle)
                    .ok_or_else(|| PluginError::invalid("节点句柄已失效"))?;
                let (owner, d) = self
                    .docs
                    .get(&doc)
                    .ok_or_else(|| PluginError::invalid("文档已失效"))?;
                if owner != scope {
                    return Err(PluginError::invalid("节点不得跨调用使用"));
                }
                let node = dom_query::NodeRef::new(node, &d.tree);
                let mut selected = None;
                let result = match string("op")?.as_str() {
                    "tag" => json!(node
                        .node_name()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "#document".into())),
                    "text" => json!(node.text().split_whitespace().collect::<Vec<_>>().join(" ")),
                    "ownText" => json!(node
                        .immediate_text()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")),
                    "attr" => json!(node.attr(&string("name")?).map(|s| s.to_string())),
                    "attrs" => json!(node
                        .attrs()
                        .iter()
                        .map(|a| (a.name.local.to_string(), a.value.to_string()))
                        .collect::<BTreeMap<_, _>>()),
                    "html" => json!(node.inner_html().to_string()),
                    "outerHtml" => json!(node.html().to_string()),
                    "select" => {
                        let sel = string("selector")?;
                        let matcher = dom_query::Matcher::new(&sel)
                            .map_err(|_| PluginError::invalid("CSS 选择器无效"))?;
                        selected = Some(
                            dom_query::Selection::from(node)
                                .select_matcher(&matcher)
                                .nodes()
                                .iter()
                                .map(|n| n.id)
                                .collect::<Vec<_>>(),
                        );
                        Value::Null
                    }
                    "children" => {
                        selected = Some(node.element_children().iter().map(|n| n.id).collect());
                        Value::Null
                    }
                    "parent" => {
                        selected = Some(node.parent().map(|n| vec![n.id]).unwrap_or_default());
                        Value::Null
                    }
                    _ => return Err(PluginError::invalid("节点操作无效")),
                };
                if let Some(ids) = selected {
                    let mut handles = vec![];
                    for id in ids {
                        self.next += 1;
                        self.nodes.insert(self.next, (doc, id));
                        handles.push(self.next);
                    }
                    if v["op"] == "parent" {
                        json!(handles.first())
                    } else {
                        json!(handles)
                    }
                } else {
                    result
                }
            }
            _ => {
                return Err(PluginError::new(
                    "UNSUPPORTED",
                    format!("宿主尚未实现 {op}"),
                ))
            }
        };
        Ok(Transfer::from_json(json_result))
    }
}
pub(crate) fn aes_transform(
    v: &Value,
    key: Vec<u8>,
    iv: Vec<u8>,
    mut data: Vec<u8>,
    aad: Vec<u8>,
    encrypt: bool,
) -> Result<Vec<u8>> {
    let mode = v["mode"]
        .as_str()
        .ok_or_else(|| PluginError::invalid("AES 缺少 mode"))?;
    let error = || PluginError::invalid("AES 参数、填充或认证标签无效");
    if !matches!(key.len(), 16 | 24 | 32) {
        return Err(error());
    }
    if mode == "gcm" {
        use aes_gcm::aead::{Aead, Payload};
        if iv.len() != 12 {
            return Err(error());
        }
        macro_rules! gcm {
            ($t:ty) => {{
                let c = <$t>::new_from_slice(&key).map_err(|_| error())?;
                let n = aes_gcm::Nonce::from_slice(&iv);
                let p = Payload {
                    msg: &data,
                    aad: &aad,
                };
                if encrypt {
                    c.encrypt(n, p)
                } else {
                    c.decrypt(n, p)
                }
                .map_err(|_| error())
            }};
        }
        return match key.len() {
            16 => gcm!(aes_gcm::Aes128Gcm),
            24 => gcm!(aes_gcm::AesGcm<aes::Aes192,aes_gcm::aead::consts::U12>),
            32 => gcm!(aes_gcm::Aes256Gcm),
            _ => Err(error()),
        };
    }
    if mode == "ctr" {
        if iv.len() != 16 {
            return Err(error());
        }
        macro_rules! ctr {
            ($t:ty) => {{
                let mut c = ctr::Ctr128BE::<$t>::new_from_slices(&key, &iv).map_err(|_| error())?;
                c.apply_keystream(&mut data);
            }};
        }
        match key.len() {
            16 => ctr!(aes::Aes128),
            24 => ctr!(aes::Aes192),
            32 => ctr!(aes::Aes256),
            _ => return Err(error()),
        };
        return Ok(data);
    }
    if !matches!(mode, "cbc" | "ecb") || mode == "cbc" && iv.len() != 16 {
        return Err(error());
    }
    let padding = v["padding"].as_str().unwrap_or("pkcs7");
    if !matches!(padding, "pkcs7" | "none") {
        return Err(error());
    }
    if encrypt && padding == "pkcs7" {
        let n = 16 - data.len() % 16;
        data.extend(std::iter::repeat_n(n as u8, n));
    }
    if data.len() % 16 != 0 {
        return Err(error());
    }
    let mut previous = if mode == "cbc" { iv } else { vec![0; 16] };
    macro_rules! blocks {
        ($t:ty) => {{
            let cipher = <$t>::new_from_slice(&key).map_err(|_| error())?;
            for block in data.chunks_mut(16) {
                let original = block.to_vec();
                if encrypt && mode == "cbc" {
                    for (i, b) in block.iter_mut().enumerate() {
                        *b ^= previous[i];
                    }
                }
                if encrypt {
                    cipher.encrypt_block(block.into());
                } else {
                    cipher.decrypt_block(block.into());
                }
                if !encrypt && mode == "cbc" {
                    for (i, b) in block.iter_mut().enumerate() {
                        *b ^= previous[i];
                    }
                }
                if mode == "cbc" {
                    previous = if encrypt { block.to_vec() } else { original };
                }
            }
        }};
    }
    match key.len() {
        16 => blocks!(aes::Aes128),
        24 => blocks!(aes::Aes192),
        32 => blocks!(aes::Aes256),
        _ => return Err(error()),
    };
    if !encrypt && padding == "pkcs7" {
        let n = *data.last().ok_or_else(error)? as usize;
        if n == 0
            || n > 16
            || data.len() < n
            || !data[data.len() - n..].iter().all(|b| *b as usize == n)
        {
            return Err(error());
        }
        data.truncate(data.len() - n);
    }
    Ok(data)
}
use std::collections::BTreeMap;
