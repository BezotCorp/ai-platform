use std::time::Duration;

use anyhow::{Result, bail};
use reqwest::{Client as HttpClient, Response, Url};
use serde_json::Value;

#[derive(Clone)]
pub(crate) struct Client {
    http: HttpClient,
    base_url: Url,
}

impl Client {
    pub(crate) fn new(url: &str) -> Result<Self> {
        let base_url = Url::parse(url)?;
        if !matches!(base_url.scheme(), "http" | "https")
            || base_url.host_str().is_none()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
            || base_url.path() != "/"
            || !base_url.username().is_empty()
            || base_url.password().is_some()
        {
            bail!("Adresse Ollama invalide");
        }
        let http = HttpClient::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(1800))
            .no_proxy()
            .build()?;
        Ok(Self { http, base_url })
    }

    pub(crate) async fn get(&self, endpoint: &str) -> Result<Value> {
        let url = self.base_url.join(&format!("api/{endpoint}"))?;
        let response = self.http.get(url).send().await?.error_for_status()?;
        Ok(response.json().await?)
    }

    pub(crate) async fn chat(&self, request: &Value) -> Result<Response> {
        let url = self.base_url.join("api/chat")?;
        Ok(self
            .http
            .post(url)
            .json(request)
            .send()
            .await?
            .error_for_status()?)
    }
}
