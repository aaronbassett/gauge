use gauge_auth::protocol::{ChallengeRequest, ChallengeResponse, VerifyRequest, VerifyResponse};
use gauge_auth::sign_challenge;
use gauge_query::{MetaResponse, QueryRequest, QueryResponse};
use serde::de::DeserializeOwned;

use crate::config::ClientConfig;
use crate::error::ClientError;
use crate::{keys, paths};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TokenCache {
    pub token: String,
    pub user_id: String,
    pub expires_at: i64,
}

impl TokenCache {
    fn save(&self) -> Result<(), ClientError> {
        let path = paths::token_path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, serde_json::to_vec(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    fn load() -> Option<TokenCache> {
        let path = paths::token_path().ok()?;
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }
}

pub struct ApiClient {
    http: reqwest::Client,
    base: String,
    user_id: String,
}

impl ApiClient {
    pub fn from_config(cfg: &ClientConfig) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("reqwest client"),
            base: cfg.server_url.clone(),
            user_id: cfg.user_id.clone(),
        }
    }

    /// Full challenge/response using the local private key; caches the JWT.
    pub async fn login(&self) -> Result<TokenCache, ClientError> {
        let kp = keys::load_keypair(&self.user_id)?;
        let ch: ChallengeResponse = self
            .post_unauthed(
                "/v1/auth/challenge",
                &ChallengeRequest {
                    user_id: self.user_id.clone(),
                },
            )
            .await?;
        let signature_b64 = sign_challenge(&kp, &ch.nonce_b64)?;
        let v: VerifyResponse = self
            .post_unauthed(
                "/v1/auth/verify",
                &VerifyRequest {
                    challenge_id: ch.challenge_id,
                    signature_b64,
                },
            )
            .await?;
        let cache = TokenCache {
            token: v.token,
            user_id: v.user_id,
            expires_at: v.expires_at,
        };
        cache.save()?;
        Ok(cache)
    }

    pub async fn query(&self, req: &QueryRequest) -> Result<QueryResponse, ClientError> {
        self.authed(
            reqwest::Method::POST,
            "/v1/query",
            Some(serde_json::to_value(req)?),
        )
        .await
    }

    pub async fn meta(&self) -> Result<MetaResponse, ClientError> {
        self.authed(reqwest::Method::GET, "/v1/meta", None).await
    }

    async fn token(&self) -> Result<String, ClientError> {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        if let Some(c) = TokenCache::load()
            && c.user_id == self.user_id
            && c.expires_at > now + 60
        {
            return Ok(c.token);
        }
        Ok(self.login().await?.token)
    }

    async fn authed<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T, ClientError> {
        let mut token = self.token().await?;
        for attempt in 0..2 {
            let mut req = self
                .http
                .request(method.clone(), format!("{}{path}", self.base))
                .bearer_auth(&token);
            if let Some(b) = &body {
                req = req.json(b);
            }
            let resp = req
                .send()
                .await
                .map_err(|e| ClientError::Http(e.to_string()))?;
            if resp.status().as_u16() == 401 && attempt == 0 {
                token = self.login().await?.token; // expired mid-session: transparent re-auth
                continue;
            }
            return Self::handle(resp).await;
        }
        unreachable!("loop always returns by attempt 1")
    }

    async fn post_unauthed<T: DeserializeOwned, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ClientError> {
        let resp = self
            .http
            .post(format!("{}{path}", self.base))
            .json(body)
            .send()
            .await
            .map_err(|e| ClientError::Http(e.to_string()))?;
        Self::handle(resp).await
    }

    async fn handle<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T, ClientError> {
        let status = resp.status().as_u16();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ClientError::Http(e.to_string()))?;
        if (200..300).contains(&status) {
            return Ok(serde_json::from_slice(&bytes)?);
        }
        #[derive(serde::Deserialize)]
        struct Envelope {
            code: String,
            message: String,
            remediation: Option<String>,
        }
        let env: Envelope = serde_json::from_slice(&bytes).unwrap_or(Envelope {
            code: "unknown".into(),
            message: format!("HTTP {status}"),
            remediation: None,
        });
        Err(ClientError::Api {
            status,
            code: env.code,
            message: env.message,
            remediation: env.remediation,
        })
    }
}
