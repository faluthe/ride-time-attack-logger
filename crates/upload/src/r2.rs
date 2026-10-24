//! Cloudflare R2 through its S3-compatible API, signed with AWS Signature Version 4.
//!
//! Needs an R2 API token with Object Read & Write on the bucket (dashboard: R2 → Manage API
//! tokens), given as environment variables or in a `.env` file (searched for from the current
//! directory up; real environment variables win):
//!
//! ```text
//! R2_ACCOUNT_ID=...          # Cloudflare account id
//! R2_ACCESS_KEY_ID=...       # from the token
//! R2_SECRET_ACCESS_KEY=...   # from the token
//! R2_BUCKET=ride-time-attack-data   # optional, this is the default
//! ```

use crate::{Object, Res, Store};
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const DEFAULT_BUCKET: &str = "ride-time-attack-data";
/// R2 takes any region in the signature; `auto` is what Cloudflare documents.
const REGION: &str = "auto";

pub struct R2 {
    agent: ureq::Agent,
    host: String,
    bucket: String,
    key_id: String,
    secret: String,
}

fn var(name: &str) -> Res<String> {
    std::env::var(name).map_err(|_| format!("{name} isn't set (environment or .env)").into())
}

impl R2 {
    /// Credentials from the environment, after loading `.env` if there is one.
    pub fn from_env() -> Res<R2> {
        dotenvy::dotenv().ok();
        let account = var("R2_ACCOUNT_ID")?;
        Ok(R2::new(
            &account,
            &std::env::var("R2_BUCKET").unwrap_or_else(|_| DEFAULT_BUCKET.into()),
            &var("R2_ACCESS_KEY_ID")?,
            &var("R2_SECRET_ACCESS_KEY")?,
        ))
    }

    pub fn new(account: &str, bucket: &str, key_id: &str, secret: &str) -> R2 {
        // errors come back as XML bodies worth showing, so don't turn statuses into errors
        let agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
        R2 {
            agent,
            host: format!("{account}.r2.cloudflarestorage.com"),
            bucket: bucket.into(),
            key_id: key_id.into(),
            secret: secret.into(),
        }
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// One signed request; returns the response body, or the status and body as an error.
    fn request(&self, method: &str, key: &str, query: &[(&str, &str)], body: &[u8], content_type: Option<&str>) -> Res<String> {
        let path = format!("/{}/{}", self.bucket, key);
        let payload = hex::encode(Sha256::digest(body));
        let date = amz_date(std::time::SystemTime::now());
        let headers = [("host", self.host.clone()), ("x-amz-content-sha256", payload.clone()), ("x-amz-date", date.clone())];
        let auth = authorization(&self.key_id, &self.secret, REGION, method, &path, query, &headers, &payload, &date);

        let qs = canonical_query(query);
        let url = format!("https://{}{}{}{}", self.host, uri_encode(&path, false), if qs.is_empty() { "" } else { "?" }, qs);
        let req = ureq::http::Request::builder()
            .method(method)
            .uri(url)
            .header("x-amz-content-sha256", payload)
            .header("x-amz-date", date)
            .header("authorization", auth);
        let req = match content_type {
            Some(t) => req.header("content-type", t),
            None => req,
        };
        let mut resp = self.agent.run(req.body(body.to_vec())?)?;
        let status = resp.status();
        let text = resp.body_mut().read_to_string()?;
        if !status.is_success() {
            return Err(format!("{method} {path}: {status} {}", text.trim()).into());
        }
        Ok(text)
    }
}

#[derive(Deserialize)]
struct ListBucketResult {
    #[serde(rename = "Contents", default)]
    contents: Vec<Contents>,
    #[serde(rename = "IsTruncated", default)]
    is_truncated: bool,
    #[serde(rename = "NextContinuationToken")]
    next_continuation_token: Option<String>,
}

#[derive(Deserialize)]
struct Contents {
    #[serde(rename = "Key")]
    key: String,
    #[serde(rename = "ETag")]
    etag: String,
}

impl Store for R2 {
    fn list(&self, prefix: &str) -> Res<Vec<Object>> {
        let mut out = vec![];
        let mut token = None::<String>;
        loop {
            let mut q = vec![("list-type", "2"), ("prefix", prefix)];
            if let Some(t) = &token {
                q.push(("continuation-token", t));
            }
            let page: ListBucketResult = quick_xml::de::from_str(&self.request("GET", "", &q, &[], None)?)?;
            out.extend(page.contents.into_iter().map(|c| Object { key: c.key, etag: c.etag.trim_matches('"').to_lowercase() }));
            match page.next_continuation_token {
                Some(t) if page.is_truncated => token = Some(t),
                _ => return Ok(out),
            }
        }
    }

    fn put(&self, key: &str, body: &[u8], content_type: &str) -> Res<()> {
        self.request("PUT", key, &[], body, Some(content_type)).map(drop)
    }

    fn delete(&self, key: &str) -> Res<()> {
        self.request("DELETE", key, &[], &[], None).map(drop)
    }
}

/// S3's URI encoding: everything but unreserved characters (and `/` in paths) as `%XX`.
fn uri_encode(s: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            b'/' if !encode_slash => out.push('/'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn canonical_query(query: &[(&str, &str)]) -> String {
    let mut q: Vec<(String, String)> = query.iter().map(|(k, v)| (uri_encode(k, true), uri_encode(v, true))).collect();
    q.sort();
    q.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")
}

fn hmac(key: &[u8], msg: &str) -> Vec<u8> {
    let mut m = Hmac::<Sha256>::new_from_slice(key).expect("hmac takes any key length");
    m.update(msg.as_bytes());
    m.finalize().into_bytes().to_vec()
}

/// The `Authorization` header for a request. `headers` are the signed ones, names lowercase;
/// `date` is `YYYYMMDDTHHMMSSZ`.
#[allow(clippy::too_many_arguments)]
fn authorization(
    key_id: &str,
    secret: &str,
    region: &str,
    method: &str,
    path: &str,
    query: &[(&str, &str)],
    headers: &[(&str, String)],
    payload_sha256: &str,
    date: &str,
) -> String {
    let mut h: Vec<&(&str, String)> = headers.iter().collect();
    h.sort_by_key(|(k, _)| *k);
    let canonical_headers: String = h.iter().map(|(k, v)| format!("{k}:{}\n", v.trim())).collect();
    let signed = h.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(";");
    let canonical = format!(
        "{method}\n{}\n{}\n{canonical_headers}\n{signed}\n{payload_sha256}",
        uri_encode(path, false),
        canonical_query(query)
    );
    let day = &date[..8];
    let scope = format!("{day}/{region}/s3/aws4_request");
    let to_sign = format!("AWS4-HMAC-SHA256\n{date}\n{scope}\n{}", hex::encode(Sha256::digest(canonical.as_bytes())));
    let mut k = hmac(format!("AWS4{secret}").as_bytes(), day);
    for part in [region, "s3", "aws4_request"] {
        k = hmac(&k, part);
    }
    let sig = hex::encode(hmac(&k, &to_sign));
    format!("AWS4-HMAC-SHA256 Credential={key_id}/{scope}, SignedHeaders={signed}, Signature={sig}")
}

/// `YYYYMMDDTHHMMSSZ` in UTC.
fn amz_date(t: std::time::SystemTime) -> String {
    let secs = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rem) = ((secs / 86400) as i64, secs % 86400);
    // days since 1970-01-01 to a civil date (Howard Hinnant's algorithm)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        let at = |s| amz_date(std::time::UNIX_EPOCH + std::time::Duration::from_secs(s));
        assert_eq!(at(0), "19700101T000000Z");
        assert_eq!(at(951_782_400), "20000229T000000Z");
        assert_eq!(at(1_792_713_600 + 3661), "20261023T010101Z");
    }

    #[test]
    fn encoding() {
        assert_eq!(uri_encode("/bucket/laps/a b+c.bin.gz", false), "/bucket/laps/a%20b%2Bc.bin.gz");
        assert_eq!(canonical_query(&[("prefix", "laps/"), ("list-type", "2")]), "list-type=2&prefix=laps%2F");
    }

    /// The GET Object example from AWS's SigV4 documentation.
    #[test]
    fn signs_like_aws_example() {
        let payload = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let headers = [
            ("x-amz-date", "20130524T000000Z".to_string()),
            ("host", "examplebucket.s3.amazonaws.com".to_string()),
            ("range", "bytes=0-9".to_string()),
            ("x-amz-content-sha256", payload.to_string()),
        ];
        let auth = authorization(
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            "us-east-1",
            "GET",
            "/test.txt",
            &[],
            &headers,
            payload,
            "20130524T000000Z",
        );
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, \
             SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, \
             Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }
}
