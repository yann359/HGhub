use base64::{engine::general_purpose::STANDARD, Engine};
use hghub_hpp::PluginManager;
use rsa::{
    pkcs1::{EncodeRsaPublicKey, RsaPublicKeyRef, UintRef},
    pkcs8::{DecodePrivateKey, EncodePublicKey, LineEnding},
    PrivateKey,
};
use serde_json::{json, Value};

// Generated solely for these tests with Node's crypto.generateKeyPairSync.
// This public fixture is never an application key or credential.
const PRIVATE_KEY: &str = include_str!("fixtures/rsa-test-only.pem.txt");
const SOURCE: &str = r#"
export const meta={id:'fixture.rsa',name:'RSA contract',version:'1.0.0',protocol:1,
  requires:[{type:'api',name:'crypto.rsaEncrypt'}]};
export default hg=>({
  media:{getDetail(){},resolvePlayback(){}},
  search:{search({query}){
    const p=JSON.parse(query);
    const data=p.raw ? p.data : hg.codec.base64Decode(p.data);
    const result=hg.crypto.rsaEncrypt(p.params,data);
    if(!(result instanceof Uint8Array)) throw new Error('Expected Uint8Array');
    // Titles are limited to 200 chars by content validation; preserve every byte.
    return {items:hg.codec.base64Encode(result).match(/.{1,100}/g).map((title,i)=>({id:String(i),title}))};
  }}
});
"#;

async fn encrypt(
    manager: &PluginManager,
    params: Value,
    data: &[u8],
) -> hghub_hpp::Result<Vec<u8>> {
    let result = manager
        .call(
            "fixture.rsa",
            "search.search",
            json!({
                "query": json!({"params":params,"data":STANDARD.encode(data)}).to_string()
            }),
            uuid::Uuid::new_v4().to_string(),
        )
        .await?;
    let encoded: String = result["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap())
        .collect();
    Ok(STANDARD.decode(encoded).unwrap())
}

#[tokio::test]
async fn rsa_pem_padding_boundaries_and_randomness_through_quickjs() {
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::open(dir.path().into()).unwrap();
    assert_eq!(
        manager
            .import_code(SOURCE.into(), None, false)
            .await
            .unwrap()
            .status,
        "ready"
    );
    let private = PrivateKey::from_pkcs8_pem(PRIVATE_KEY).unwrap();
    let public = private.to_public_key();
    for pem in [
        public.to_public_key_pem(LineEnding::LF).unwrap(),
        public.to_pkcs1_pem(LineEnding::CRLF).unwrap(),
    ] {
        for (padding, max) in [("pkcs1", 245), ("oaep-sha1", 214), ("oaep-sha256", 190)] {
            let params = json!({"publicKey":pem,"padding":padding});
            for len in [0, 1, max] {
                // Includes zero and non-UTF8 bytes at the maximum size.
                let data: Vec<u8> = (0..len).map(|i| (i * 131) as u8).collect();
                let ciphertext = encrypt(&manager, params.clone(), &data).await.unwrap();
                assert_eq!(ciphertext.len(), 256);
                let plaintext = match padding {
                    "pkcs1" => private.pkcs1v15_decrypt(&ciphertext),
                    "oaep-sha1" => private.oaep_decrypt::<rsa_sha1::Sha1>(&ciphertext),
                    _ => private.oaep_decrypt::<rsa::sha2::Sha256>(&ciphertext),
                }
                .unwrap();
                assert_eq!(plaintext, data, "{padding}/{len}");
                assert_ne!(
                    ciphertext,
                    encrypt(&manager, params.clone(), &data).await.unwrap()
                );
            }
            assert_eq!(
                encrypt(&manager, params, &vec![0; max + 1])
                    .await
                    .unwrap_err()
                    .code,
                "INVALID_ARGUMENT"
            );
        }
    }
}

#[tokio::test]
async fn rsa_invalid_inputs_and_unknown_requirements_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::open(dir.path().into()).unwrap();
    assert_eq!(
        manager
            .import_code(SOURCE.into(), None, false)
            .await
            .unwrap()
            .status,
        "ready"
    );
    let pem = PrivateKey::from_pkcs8_pem(PRIVATE_KEY)
        .unwrap()
        .to_public_key()
        .to_public_key_pem(LineEnding::LF)
        .unwrap();
    for params in [
        json!(null),
        json!({}),
        json!({"publicKey":pem}),
        json!({"publicKey":pem,"padding":"oaep"}),
        json!({"publicKey":pem,"padding":1}),
        json!({"publicKey":17,"padding":"pkcs1"}),
        json!({"publicKey":"broken pem","padding":"pkcs1"}),
        json!({"publicKey":PRIVATE_KEY,"padding":"pkcs1"}),
        json!({"publicKey":pem.replace("MIIB", "!!!!"),"padding":"pkcs1"}),
    ] {
        assert_eq!(
            encrypt(&manager, params, b"secret-input")
                .await
                .unwrap_err()
                .code,
            "INVALID_ARGUMENT"
        );
    }
    for data in [
        json!(null),
        json!("text"),
        json!([1, 2]),
        json!({"0":1}),
        json!(123),
    ] {
        let failure = manager
            .call(
                "fixture.rsa",
                "search.search",
                json!({"query":json!({
            "params":{"publicKey":pem,"padding":"pkcs1"},"raw":true,"data":data
        }).to_string()}),
                uuid::Uuid::new_v4().to_string(),
            )
            .await
            .unwrap_err();
        assert_eq!(failure.code, "INVALID_ARGUMENT");
        assert!(!failure.message.contains("secret-input"));
        assert!(!failure.message.contains("BEGIN"));
    }
    let unknown = SOURCE
        .replace("fixture.rsa", "fixture.unknown")
        .replace("crypto.rsaEncrypt'}", "crypto.rsaDecrypt'}");
    assert_eq!(
        manager
            .import_code(unknown, None, false)
            .await
            .unwrap()
            .status,
        "incompatible"
    );
}

#[tokio::test]
async fn rsa_key_size_limits_are_checked_before_encryption() {
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::open(dir.path().into()).unwrap();
    assert_eq!(
        manager
            .import_code(SOURCE.into(), None, false)
            .await
            .unwrap()
            .status,
        "ready"
    );
    // Synthetic odd moduli test parsing/limits without generating large private keys.
    for bits in [512usize, 1023, 1024, 8192, 8193] {
        let mut modulus = vec![0; bits.div_ceil(8)];
        modulus[0] = 1 << ((bits - 1) % 8);
        *modulus.last_mut().unwrap() = 1;
        let pem = RsaPublicKeyRef {
            modulus: UintRef::new(&modulus).unwrap(),
            public_exponent: UintRef::new(&[1, 0, 1]).unwrap(),
        }
        .to_pkcs1_pem(LineEnding::LF)
        .unwrap();
        let result = encrypt(
            &manager,
            json!({"publicKey":pem,"padding":"oaep-sha256"}),
            b"limit",
        )
        .await;
        if (1024..=8192).contains(&bits) {
            assert_eq!(result.unwrap().len(), bits.div_ceil(8));
        } else {
            assert_eq!(result.unwrap_err().code, "INVALID_ARGUMENT", "{bits}");
        }
    }
}
