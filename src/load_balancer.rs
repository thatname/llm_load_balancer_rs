use crate::models::{Config, ModelInfo, Provider};
use anyhow::{Context, Result};
use dashmap::DashMap;
use reqwest::Client;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

pub struct LoadBalancer {
    config: Config,
    current_provider_index: Arc<AtomicUsize>,
    model_to_provider: Arc<DashMap<String, usize>>,
    provider_models: Arc<DashMap<usize, Vec<ModelInfo>>>,
    http_client: Client,
    default_model: Arc<RwLock<Option<String>>>,
}

impl LoadBalancer {
    pub async fn new(config_path: &str) -> Result<Self> {
        let config = Self::load_config(config_path)?;
        let http_client = Client::builder()
            .timeout(std::time::Duration::from_secs(300)) // Increase timeout for long LLM responses
            .build()
            .context("Failed to create HTTP client")?;

        let load_balancer = Self {
            config,
            current_provider_index: Arc::new(AtomicUsize::new(0)),
            model_to_provider: Arc::new(DashMap::new()),
            provider_models: Arc::new(DashMap::new()),
            http_client,
            default_model: Arc::new(RwLock::new(None)),
        };

        load_balancer.fetch_all_models().await?;
        Ok(load_balancer)
    }

    fn load_config(config_path: &str) -> Result<Config> {
        let content = std::fs::read_to_string(config_path)
            .context(format!("Failed to read config file: {}", config_path))?;
        
        serde_yaml::from_str(&content).context("Failed to parse config YAML")
    }

    async fn fetch_all_models(&self) -> Result<()> {
        info!("============================================================");
        info!("LLM Load Balancer - Fetching supported models from providers");
        info!("============================================================");

        let tasks: Vec<_> = self
            .config
            .providers
            .iter()
            .enumerate()
            .map(|(index, provider)| {
                let client = self.http_client.clone();
                let provider = provider.clone();
                async move {
                    (index, Self::fetch_provider_models(client, &provider).await)
                }
            })
            .collect();

        let results = futures::future::join_all(tasks).await;

        for (index, result) in results {
            let provider = &self.config.providers[index];
            match result {
                Ok(models) => {
                    info!("\nProvider {}: {}", index + 1, provider.base_url);
                    info!("  Supported models ({}):", models.len());
                    for model in &models {
                        info!("    - {}", model.id);
                        // Store model-to-provider mapping
                        self.model_to_provider.insert(model.id.clone(), index);
                    }
                    self.provider_models.insert(index, models);
                }
                Err(e) => {
                    warn!("\nProvider {}: {}", index + 1, provider.base_url);
                    warn!("  Error: {}", e);
                    self.provider_models.insert(index, Vec::new());
                }
            }
        }

        info!("\n============================================================");
        info!("LLM Load Balancer startup completed");
        info!("============================================================\n");

        Ok(())
    }

    async fn fetch_provider_models(
        client: Client,
        provider: &Provider,
    ) -> Result<Vec<ModelInfo>> {
        let url = format!("{}/models", provider.base_url);
        
        debug!("Fetching models from: {}", url);

        let response = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", provider.key))
            .send()
            .await
            .context("Failed to send request to /models endpoint")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            anyhow::bail!("Provider returned status {}: {}", status, error_text);
        }

        let models_response: serde_json::Value = response.json().await
            .context("Failed to parse models response")?;

        let models: Vec<ModelInfo> = serde_json::from_value(
            models_response.get("data").cloned().unwrap_or(serde_json::Value::Array(vec![]))
        ).context("Failed to parse models list")?;

        Ok(models)
    }

    pub fn get_current_provider_index(&self) -> usize {
        self.current_provider_index.load(Ordering::Relaxed) % self.config.providers.len()
    }

    pub fn advance_provider_index(&self) {
        self.current_provider_index.fetch_add(1, Ordering::Relaxed);
    }

    pub fn find_provider_for_model(&self, model_name: &str) -> Option<usize> {
        // Check if it's the default model
        if model_name == "default" {
            return None; // Will be handled separately
        }

        // Exact match
        if let Some(index) = self.model_to_provider.get(model_name) {
            return Some(*index);
        }

        None
    }

    pub fn get_available_models(&self) -> Vec<(String, usize)> {
        self.model_to_provider
            .iter()
            .map(|entry| (entry.key().clone(), *entry.value()))
            .collect()
    }

    pub async fn get_default_model(&self) -> Option<String> {
        self.default_model.read().await.clone()
    }

    pub async fn set_default_model(&self, model: Option<String>) {
        if let Some(ref model) = model {
            info!("Default model set to: {}", model);
        } else {
            info!("Default model cleared");
        }
        *self.default_model.write().await = model;
    }

    pub fn get_config(&self) -> &Config {
        &self.config
    }

    pub fn get_http_client(&self) -> &Client {
        &self.http_client
    }

    pub fn get_provider(&self, index: usize) -> Option<&Provider> {
        self.config.providers.get(index)
    }

    pub fn get_provider_models(&self, index: usize) -> Option<Vec<ModelInfo>> {
        self.provider_models.get(&index).map(|v| v.clone())
    }
}
