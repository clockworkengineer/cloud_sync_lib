use std::io;
use std::pin::Pin;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use futures::io::AsyncRead;
use futures::TryStreamExt;
use json::Value;
use reqwest::Body;
use tokio_util::codec::{BytesCodec, FramedRead};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use url::Url;

use crate::commands::{Request, Response};
use crate::error::Error;
use crate::http::HttpClient;
use crate::{ClientState, ErrorCode};

#[async_trait]
impl HttpClient for reqwest::Client {
    async fn send_requests(
        &self,
        state: &ClientState,
        requests: &[Request],
        query_params: &[(&str, &str)],
    ) -> Result<Vec<Response>, Error> {
        let url = {
            let mut url = state.origin.join("/cs").unwrap();

            let mut qs = url.query_pairs_mut();
            let id_counter = state.id_counter.fetch_add(1, Ordering::SeqCst);
            qs.append_pair("id", id_counter.to_string().as_str());

            if let Some(session) = state.session.as_ref() {
                qs.append_pair("sid", session.sid.as_str());
            }

            qs.extend_pairs(query_params);

            qs.finish();
            drop(qs);

            url
        };

        let mut delay = state.min_retry_delay;
        for i in 0..state.max_retries {
            if i > 0 {
                tokio::time::sleep(delay).await;
                delay *= 2;
                // TODO: maybe add some small random jitter after the doubling.
                if delay > state.max_retry_delay {
                    delay = state.max_retry_delay;
                }
            }

            // dbg!(&requests);
            let request = self.post(url.clone()).json(requests).send();
            let maybe_response = if let Some(timeout) = state.timeout {
                let Ok(maybe_response) = tokio::time::timeout(timeout, request).await else {
                    // the timeout has been reached, let's retry.
                    continue;
                };
                maybe_response
            } else {
                request.await
            };

            let Ok(response) = maybe_response else {
                // this could be a network issue, let's retry.
                continue;
            };

            let response = if response.status().as_u16() == 402 {
                if let Some(hc) = response
                    .headers()
                    .get("X-Hashcash")
                    .and_then(|h| h.to_str().ok())
                {
                    if let Some((easiness, token)) = parse_hashcash(hc) {
                        if let Some(cash_value) = solve_hashcash(&token, easiness) {
                            let hc_header_val = format!("1:{}:{}", token, cash_value);
                            let req = self
                                .post(url.clone())
                                .header("X-Hashcash", hc_header_val)
                                .json(requests)
                                .send();
                            let retry_resp = if let Some(timeout) = state.timeout {
                                tokio::time::timeout(timeout, req).await.ok().and_then(|r| r.ok())
                            } else {
                                req.await.ok()
                            };
                            match retry_resp {
                                Some(r) => r,
                                None => continue,
                            }
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            } else {
                response
            };

            if !response.status().is_success() {
                // this could be a server error, let's retry.
                continue;
            }

            let Ok(response) = response.bytes().await else {
                // this could be a network issue, let's retry.
                continue;
            };

            // try to parse a request-level error first.
            if let Ok(code) = json::from_slice::<ErrorCode>(&response) {
                if code == ErrorCode::EAGAIN {
                    // this error code suggests we might succeed if retried, let's retry.
                    continue;
                }
                return Err(Error::from(code));
            }

            let responses: Vec<Value> = json::from_slice(&response)?;
            // dbg!(&responses);

            return requests
                .iter()
                .zip(responses)
                .map(|(request, response)| request.parse_response_data(response))
                .collect();
        }

        Err(Error::MaxRetriesReached)
    }

    async fn get(&self, url: Url) -> Result<Pin<Box<dyn AsyncRead>>, Error> {
        let stream = self
            .get(url)
            .send()
            .await?
            .bytes_stream()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err));

        Ok(Box::pin(stream.into_async_read()))
    }

    async fn post(
        &self,
        url: Url,
        body: Pin<Box<dyn AsyncRead + Send + Sync>>,
        content_length: Option<u64>,
    ) -> Result<Pin<Box<dyn AsyncRead>>, Error> {
        let stream = FramedRead::new(body.compat(), BytesCodec::new());
        let body = Body::wrap_stream(stream);
        let stream = {
            let mut builder = self.post(url);

            if let Some(content_length) = content_length {
                builder = builder.header("content-length", content_length);
            }

            builder
                .body(body)
                .send()
                .await?
                .bytes_stream()
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
        };

        Ok(Box::pin(stream.into_async_read()))
    }
}

fn parse_hashcash(header: &str) -> Option<(u8, String)> {
    let parts: Vec<&str> = header.split(':').collect();
    if parts.len() != 4 {
        return None;
    }
    let v: u32 = parts[0].parse().ok()?;
    if v != 1 {
        return None;
    }
    let e: u8 = parts[1].parse().ok()?;
    let token = parts[3].to_string();
    Some((e, token))
}

fn solve_hashcash(token: &str, easiness: u8) -> Option<String> {
    use base64::prelude::{Engine, BASE64_URL_SAFE_NO_PAD};
    use sha2::{Digest, Sha256};

    let threshold: u32 = ((((easiness as u32 & 63) << 1) + 1) as u32)
        << (((easiness as u32 >> 6) * 7) + 3);

    let mut token_bytes = BASE64_URL_SAFE_NO_PAD.decode(token).ok()?;
    let rem = token_bytes.len() % 16;
    if rem != 0 {
        token_bytes.resize(token_bytes.len() + 16 - rem, 0);
    }

    const NUM_REPLICATIONS: usize = 262144;
    const TOKEN_SLOT_SIZE: usize = 48;
    let mut buffer = vec![0u8; 4 + NUM_REPLICATIONS * TOKEN_SLOT_SIZE];

    for i in 0..NUM_REPLICATIONS {
        let start = 4 + i * TOKEN_SLOT_SIZE;
        buffer[start..start + token_bytes.len()].copy_from_slice(&token_bytes);
    }

    for counter in 1u32..=u32::MAX {
        buffer[..4].copy_from_slice(&counter.to_le_bytes());
        let hash = Sha256::digest(&buffer);
        let hash_val = u32::from_be_bytes(hash[..4].try_into().unwrap());
        if hash_val <= threshold {
            return Some(BASE64_URL_SAFE_NO_PAD.encode(&counter.to_le_bytes()));
        }
    }

    None
}

