mod api;
mod chat_ui;
mod image_utils;
mod load_balancer;
mod models;

use api::{
    anthropic_messages_handler, chat_completions_handler, health_handler, models_handler,
    openai_models_handler, set_default_get_handler, set_default_post_handler, AppState,
};
use axum::{
    routing::{get, post},
    Router,
};
use chat_ui::chat_ui_handler;
use load_balancer::LoadBalancer;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "llm_load_balancer=debug,tower_http=debug,axum=trace".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Get config path from command line args or use default
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../openrouter.yaml".to_string());

    info!("Loading configuration from: {}", config_path);

    // Initialize load balancer
    let load_balancer = Arc::new(LoadBalancer::new(&config_path).await?);
    let port = load_balancer.get_config().port;

    // Create app state
    let app_state = AppState {
        load_balancer: load_balancer.clone(),
    };

    // Build our application with CORS
    let app = Router::new()
        .route("/v1/chat/completions", post(chat_completions_handler))
        .route("/v1/messages", post(anthropic_messages_handler))
        .route("/v1/models", get(openai_models_handler))
        .route("/models", get(models_handler))
        .route("/health", get(health_handler))
        .route("/set_default", get(set_default_get_handler))
        .route("/set_default", post(set_default_post_handler))
        .route("/chat", get(chat_ui_handler))
        .layer(
            ServiceBuilder::new()
                .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any)),
        )
        .with_state(app_state);

    info!("Starting LLM Load Balancer server on port {}", port);

    // Create TCP listener
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!("Server listening on {}", listener.local_addr()?);

    // Run the web server in a background task
    tokio::spawn(async move {
        info!("Server running.");
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await {
                error!("Server error: {}", e);
            }
        info!("Server stopped.");
    });

    // Open browser to /chat page
    let url = format!("http://localhost:{}", port);
    info!("Opening browser to: {}", url);
    
    if let Err(e) = open::that(format!("{}/chat", url)) {
        error!("Failed to open browser: {}", e);
    }

    // Wait for shutdown signal
    shutdown_signal().await;
    info!("Server shutting down...");

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C signal");
        },
        _ = terminate => {
            info!("Received terminate signal");
        },
    }
}