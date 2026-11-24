use super::*;

const SITE: &str = "https://accounts.example.invalid";
const PAGE: &str = "https://accounts.example.invalid/me";
fn network(dir: &std::path::Path) -> Network {
    let meta = Meta::parse(
        json!({"id":"fixture.web-login","protocol":1,"name":"fixture","version":"1.0.0"}),
    )
    .unwrap();
    Network::new(meta, dir.into(), Some(false), Default::default()).unwrap()
}
fn marker(value: &str) -> Value {
    json!({"name":"session", "value":value, "domain":"example.invalid", "hostOnly":false,
          "path":"/me", "secure":true, "httpOnly":true})
}
fn rotation() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.append(
        "set-cookie",
        HeaderValue::from_static("rotated=two; Path=/me; Secure; HttpOnly"),
    );
    headers.append(
        "set-cookie",
        HeaderValue::from_static("session=; Max-Age=0; Path=/me; Domain=example.invalid; Secure"),
    );
    headers
}

#[test]
fn only_native_requests_use_web_login_and_credentials_stay_in_scope() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let net = network(dir.path());
    let page = Url::parse(PAGE).unwrap();
    let ordinary =
        json!({"url":PAGE,"cookie":{"name":"session","value":"plugin-value","httpOnly":true}});
    net.cookies("set", ordinary.clone()).unwrap();
    assert_eq!(net.request_cookies(&page, true).1, "session=plugin-value");
    // Reload an existing ordinary array before the first native login.
    drop(net);
    let net = network(dir.path());
    net.replace_web_login(SITE, PAGE, vec![marker("one")])
        .unwrap();
    net.cookies("set", ordinary).unwrap();
    assert_eq!(
        net.cookies("get", json!({"url":PAGE})).unwrap()[0]["value"],
        "plugin-value"
    );
    assert_eq!(net.request_cookies(&page, true).1, "session=one");
    assert_eq!(net.request_cookies(&page, false).1, "");
    for url in [
        "https://cdn.example.invalid/me",
        "https://accounts.example.invalid:444/me",
        "http://accounts.example.invalid/me",
        "https://accounts.example.invalid/menu",
    ] {
        assert!(!net
            .request_cookies(&Url::parse(url).unwrap(), true)
            .1
            .contains("session=one"));
    }
    assert_eq!(network(other.path()).request_cookies(&page, true).1, "");
    let generation = net.request_cookies(&page, true).0.unwrap();
    let mut response = rotation();
    assert!(net
        .receive_cookies(&page, &mut response, Some(&generation), true)
        .unwrap());
    assert!(!response.contains_key("set-cookie"));
    assert_eq!(net.request_cookies(&page, true).1, "rotated=two");
    let mut omitted = rotation();
    assert!(net
        .receive_cookies(&page, &mut omitted, Some(&generation), false)
        .unwrap());
    assert!(!omitted.contains_key("set-cookie"));
    drop(net);
    let net = network(dir.path());
    assert_eq!(net.request_cookies(&page, true).1, "rotated=two");
    net.replace_web_login(SITE, PAGE, vec![marker("new-account")])
        .unwrap();
    assert!(net
        .receive_cookies(&page, &mut rotation(), Some(&generation), true)
        .unwrap());
    assert_eq!(net.request_cookies(&page, true).1, "session=new-account");
    net.cookies("clear", json!({"url":SITE})).unwrap();
    assert!(net
        .receive_cookies(&page, &mut rotation(), Some(&generation), true)
        .unwrap());
    assert_eq!(net.request_cookies(&page, true).1, "");
    assert_eq!(
        network(dir.path())
            .cookies("get", json!({"url":PAGE}))
            .unwrap(),
        json!([])
    );
}

#[test]
fn rejects_cross_origin_expired_or_incomplete_snapshot_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let net = network(dir.path());
    let page = Url::parse(PAGE).unwrap();
    net.replace_web_login(SITE, PAGE, vec![marker("original")])
        .unwrap();
    assert!(net
        .replace_web_login(
            SITE,
            "https://other.example.invalid/me",
            vec![marker("new")]
        )
        .is_err());
    for (key, bad) in [
        ("expiresAt", json!(1)),
        ("hostOnly", Value::Null),
        ("domain", json!("foreign.invalid")),
        ("path", json!("/members")),
    ] {
        let mut invalid = marker("bad");
        invalid[key] = bad;
        assert!(net
            .replace_web_login(SITE, PAGE, vec![marker("new"), invalid])
            .is_err());
    }
    assert_eq!(net.request_cookies(&page, true).1, "session=original");
    net.cookies("clear", json!({})).unwrap();
    assert!(net.request_cookies(&page, true).0.is_none());
}
