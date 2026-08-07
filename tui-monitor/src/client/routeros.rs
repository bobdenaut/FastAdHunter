//! RouterOS REST, read-only.
//!
//! **Only GETs are issued, and only these two.** The router is the household's
//! live gateway (root CLAUDE.md §The router is off limits); this monitor
//! displays it and never writes to it.

use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::config::RouterOsConfig;
use crate::models::routeros::{Container, OneOrMany, SystemResource};

pub struct RouterOsClient {
    http: reqwest::Client,
    base: String,
    user: String,
    password: String,
    /// Which container's `memory-current` is reported.
    container: String,
}

impl RouterOsClient {
    pub fn new(
        base: &str,
        config: &RouterOsConfig,
        timeout: Duration,
    ) -> Result<Self, reqwest::Error> {
        Ok(Self {
            http: reqwest::Client::builder()
                // RouterOS serves its own self-signed certificate, on a device
                // whose identity is the LAN address it is reached at.
                .danger_accept_invalid_certs(true)
                .timeout(timeout)
                .build()?,
            base: base.trim_end_matches('/').to_string(),
            user: config.user.clone(),
            password: config.resolved_password(),
            container: config.container.clone(),
        })
    }

    pub async fn system_resource(&self) -> Result<SystemResource, String> {
        let body: OneOrMany<SystemResource> = self.get("/system/resource").await?;
        body.into_first()
            .ok_or_else(|| "empty /system/resource".to_string())
    }

    /// The configured container, if the router is running one by that name.
    pub async fn container(&self) -> Result<Option<Container>, String> {
        let containers: Vec<Container> = self.get("/container").await?;
        Ok(containers
            .into_iter()
            .find(|entry| entry.name.as_deref() == Some(self.container.as_str())))
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        self.http
            .get(format!("{}{path}", self.base))
            .basic_auth(&self.user, Some(&self.password))
            .send()
            .await
            .map_err(|err| err.to_string())?
            .error_for_status()
            .map_err(|err| err.to_string())?
            .json()
            .await
            .map_err(|err| err.to_string())
    }
}
