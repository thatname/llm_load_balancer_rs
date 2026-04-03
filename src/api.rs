use crate::image_utils::convert_webp_images_to_jpeg;
use crate::load_balancer::LoadBalancer;
use crate::models::{
    AnthropicContent, AnthropicRequest, AnthropicResponse, AnthropicResponseContent,
    AnthropicSystem, AnthropicTool, AnthropicToolChoice, AnthropicUsage, ErrorResponse,
    OpenAIResponse,
};
use axum::{
    body::Body,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

#[derive(Clone)]
pub struct AppState {
    pub load_balancer: Arc<LoadBalancer>,
}

pub async fn chat_completions_handler(
    State(state): State<AppState>,
    body: Json<Value>,
) -> Response {
    let mut body = body.0;
    let load_balancer = &state.load_balancer;

    // Convert WebP images to JPEG before processing
    let has_webp = body.to_string().contains("data:image/webp;base64,");
    if has_webp {
        debug!("Detected WebP images in request, converting to JPEG");
        body = convert_webp_images_to_jpeg(&body);
    }

    let model_name = match body.get("model").and_then(|v| v.as_str()) {
        Some(name) => name,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Missing 'model' field in request".to_string(),
                }),
            )
                .into_response();
        }
    };

    tracing::info!("Received chat completion request for model: {}", model_name);

    let is_streaming = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut last_error = None;

    // Check model support
    let selected_model = if model_name == "default" {
        load_balancer.get_default_model().await
    } else {
        Some(model_name.to_string())
    };

    let selected_model = match selected_model {
        Some(m) => m,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "No default model found".to_string(),
                }),
            )
                .into_response();
        }
    };

    // Verify model exists in any provider, if not, use default model
    let final_model = if load_balancer.find_provider_for_model(&selected_model).is_none() {
        tracing::warn!(
            "Model '{}' not found in any provider, falling back to default model",
            selected_model
        );
        match load_balancer.get_default_model().await {
            Some(default_model) => {
                tracing::info!("Using default model: {}", default_model);
                default_model
            }
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: format!(
                            "Model '{}' not found and no default model configured",
                            selected_model
                        ),
                    }),
                )
                    .into_response();
            }
        }
    } else {
        selected_model
    };

    // Try all providers with load balancing logic
    let num_providers = load_balancer.get_config().providers.len();

    // Check if we know which provider supports this model
    let known_provider_index = load_balancer.find_provider_for_model(&final_model);

    for _attempt in 0..num_providers {
        let provider_idx = load_balancer.get_current_provider_index();

        // Skip providers that don't support this model (if we have that information)
        if let Some(known_idx) = known_provider_index {
            if provider_idx != known_idx {
                tracing::debug!(
                    "Skipping provider {} as it doesn't support model '{}'",
                    provider_idx,
                    final_model
                );
                load_balancer.advance_provider_index();
                continue;
            }
        }

        let provider = match load_balancer.get_provider(provider_idx) {
            Some(p) => p,
            None => {
                last_error = Some(format!("Provider {} not found", provider_idx));
                continue;
            }
        };

        tracing::info!(
            "Attempting provider {}: {}",
            provider_idx,
            provider.base_url
        );

        // Build the request body with provider-specific extra params
        let mut request_body = body.clone();

        // Replace the model with the final model (either the requested model or default)
        request_body["model"] = serde_json::json!(final_model);

        for (key, value) in &provider.extra_params {
            if key != "base_url" && key != "key" && key != "model_regex" {
                if let Some(value) = serde_json::to_value(value).ok() {
                    request_body[key] = value;
                }
            }
        }

        // Forward the request to the provider
        let url = format!("{}/chat/completions", provider.base_url);
        let client = load_balancer.get_http_client();
        let request_builder = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", provider.key))
            .json(&request_body);

        match request_builder.send().await {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    tracing::info!(
                        "Successfully forwarded request to provider {}",
                        provider_idx
                    );

                    if is_streaming {
                        let mut response_builder = Response::builder()
                            .status(StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK));

                        // Copy all headers from the provider response to the client response
                        if let Some(headers) = response_builder.headers_mut() {
                            for (name, value) in response.headers() {
                                if name != "content-length" && name != "transfer-encoding" {
                                    headers.insert(name, value.clone());
                                }
                            }
                        }

                        let stream = response.bytes_stream();
                        return response_builder
                            .body(Body::from_stream(stream))
                            .unwrap()
                            .into_response();
                    }

                    // Get the response body as bytes
                    let body_bytes = match response.bytes().await {
                        Ok(bytes) => bytes.to_vec(),
                        Err(e) => {
                            last_error = Some(format!(
                                "Failed to read response body from provider {}: {}",
                                provider_idx, e
                            ));
                            continue;
                        }
                    };

                    // Return the response
                    return Response::builder()
                        .status(StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK))
                        .header("content-type", "application/json")
                        .body(Body::from(body_bytes))
                        .unwrap()
                        .into_response();
                } else {
                    // Provider returned an error
                    let error_text = match response.text().await {
                        Ok(text) => text,
                        Err(_) => "Unknown error".to_string(),
                    };
                    last_error = Some(format!(
                        "Provider {} returned status {}: {}",
                        provider_idx, status, error_text
                    ));
                    error!(
                        "Provider {} failed with status {}: {}",
                        provider_idx, status, error_text
                    );
                }
            }
            Err(e) => {
                last_error = Some(format!("Request to provider {} failed: {:?}", provider_idx, e));
                error!("Provider {} failed with exception: {:?}", provider_idx, e);
            }
        }

        // Advance to the next provider for the next attempt/request.
        load_balancer.advance_provider_index();
    }

    // All providers failed
    let error_msg = last_error.unwrap_or_else(|| "Unknown error".to_string());
    error!("All providers failed. Last error: {}", error_msg);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: format!("All providers failed. Last error: {}", error_msg),
        }),
    )
        .into_response()
}

pub async fn models_handler(State(state): State<AppState>) -> Response {
    let load_balancer = &state.load_balancer;
    let config = load_balancer.get_config();

    let mut formatted_models = Vec::new();
    for (index, provider) in config.providers.iter().enumerate() {
        let models = load_balancer.get_provider_models(index).unwrap_or_default();
        let provider_info = serde_json::json!({
            "provider_index": index,
            "base_url": provider.base_url,
            "models": models.iter().map(|model| {
                serde_json::json!({
                    "id": model.id,
                    "object": model.object.as_deref().unwrap_or("model"),
                    "created": model.created,
                    "owned_by": model.owned_by.as_deref().unwrap_or("Unknown")
                })
            }).collect::<Vec<_>>()
        });
        formatted_models.push(provider_info);
    }

    Json(serde_json::json!({
        "data": formatted_models,
        "object": "list",
        "total_providers": config.providers.len()
    }))
    .into_response()
}

/// OpenAI-compatible /v1/models endpoint
/// Returns models in OpenAI API format with default model first
pub async fn openai_models_handler(State(state): State<AppState>) -> Response {
    let load_balancer = &state.load_balancer;
    let default_model = load_balancer.get_default_model().await;

    // Get all available models
    let all_models = load_balancer.get_available_models();
    let mut models_data = Vec::new();

    // If there's a default model, add it first
    if let Some(ref default) = default_model {
        // Find the provider for the default model to get full model info
        if let Some(provider_idx) = load_balancer.find_provider_for_model(default) {
            if let Some(provider_models) = load_balancer.get_provider_models(provider_idx) {
                if let Some(model_info) = provider_models.iter().find(|m| &m.id == default) {
                    models_data.push(serde_json::json!({
                        "id": model_info.id,
                        "object": model_info.object.as_deref().unwrap_or("model"),
                        "created": model_info.created,
                        "owned_by": model_info.owned_by.as_deref().unwrap_or("Unknown")
                    }));
                }
            }
        }
    }

    // Add all other models
    for (model_name, provider_idx) in all_models {
        // Skip if this is the default model (already added)
        if let Some(ref default) = default_model {
            if &model_name == default {
                continue;
            }
        }

        // Get full model info from provider
        if let Some(provider_models) = load_balancer.get_provider_models(provider_idx) {
            if let Some(model_info) = provider_models.iter().find(|m| m.id == model_name) {
                models_data.push(serde_json::json!({
                    "id": model_info.id,
                    "object": model_info.object.as_deref().unwrap_or("model"),
                    "created": model_info.created,
                    "owned_by": model_info.owned_by.as_deref().unwrap_or("Unknown")
                }));
            }
        }
    }

    Json(serde_json::json!({
        "object": "list",
        "data": models_data
    }))
    .into_response()
}

pub async fn health_handler() -> Response {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "LLM Load Balancer"
    }))
    .into_response()
}

pub async fn refresh_models_handler(State(state): State<AppState>) -> Response {
    let load_balancer = &state.load_balancer;

    match load_balancer.refresh_models().await {
        Ok(_) => {
            tracing::info!("Successfully refreshed models from all providers");
            Json(serde_json::json!({
                "success": true,
                "message": "Models refreshed successfully"
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to refresh models: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "success": false,
                    "error": format!("Failed to refresh models: {}", e)
                })),
            )
                .into_response()
        }
    }
}

use axum::response::Html;
use axum::extract::Form;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct SetDefaultRequest {
    pub default_model: String,
}

pub async fn set_default_get_handler(State(state): State<AppState>) -> impl IntoResponse {
    let load_balancer = &state.load_balancer;

    let mut available_models = load_balancer.get_available_models();
    let current_default = load_balancer.get_default_model().await;

    // Sort models alphabetically
    available_models.sort_by(|a, b| a.0.cmp(&b.0));

    let mut models_html = String::new();

    // Add "None" option
    let none_selected = if current_default.is_none() {
        "checked"
    } else {
        ""
    };
    models_html.push_str(&format!(
        r#"<div class="model-option">
    <input type="radio" id="none" name="default_model" value="" {}>
    <label for="none">
        <div class="model-name">None</div>
    </label>
</div>"#,
        none_selected
    ));

    // Add each model with its provider URL
    for (model_name, provider_index) in available_models {
        let provider_url = if let Some(provider) = load_balancer.get_provider(provider_index) {
            &provider.base_url
        } else {
            "Unknown"
        };

        let selected = if current_default.as_deref() == Some(&model_name) {
            "checked"
        } else {
            ""
        };

        models_html.push_str(&format!(
            r#"<div class="model-option">
    <input type="radio" id="{}" name="default_model" value="{}" {}>
    <label for="{}">
        <div class="model-name">{}</div>
        <div class="provider-url">Provider: {}</div>
    </label>
</div>"#,
            model_name.replace("/", "_"),
            model_name,
            selected,
            model_name.replace("/", "_"),
            model_name,
            provider_url
        ));
    }

    let html = format!(
        r#"
<!DOCTYPE html>
<html>
<head>
    <title>LLM Load Balancer - Set Default Model</title>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
            display: flex;
            justify-content: center;
            align-items: flex-start;
            min-height: 100vh;
            margin: 0;
            padding: 2rem;
            background-color: #f5f7fa;
        }}
        .container {{
            background: white;
            padding: 2rem;
            border-radius: 12px;
            box-shadow: 0 4px 6px rgba(0,0,0,0.1);
            width: 100%;
            height: 100%;
            margin-top: 2rem;
        }}
        h1 {{
            margin-top: 0;
            margin-bottom: 1.5rem;
            font-size: 1.5rem;
            color: #1a202c;
            border-bottom: 2px solid #e2e8f0;
            padding-bottom: 1rem;
        }}
        .filter-container {{
            margin-bottom: 1.5rem;
        }}
        #model-filter {{
            width: 100%;
            padding: 0.75rem;
            border: 2px solid #e2e8f0;
            border-radius: 6px;
            font-size: 1rem;
            font-family: inherit;
            transition: border-color 0.2s, box-shadow 0.2s;
            box-sizing: border-box;
        }}
        #model-filter:focus {{
            outline: none;
            border-color: #3182ce;
            box-shadow: 0 0 0 3px rgba(49, 130, 206, 0.1);
        }}
        #model-filter::placeholder {{
            color: #a0aec0;
        }}
        .models-list {{
            max-height: 850px;
            overflow-y: auto;
            border: 1px solid #e2e8f0;
            border-radius: 8px;
            padding: 1rem;
            margin-bottom: 1.5rem;
            background-color: #fafafa;
        }}
        .model-option {{
            margin-bottom: 0.5rem;
            padding: 0.5rem 0.75rem;
            border: 1px solid #e2e8f0;
            border-radius: 6px;
            background-color: white;
            transition: background-color 0.2s, border-color 0.2s;
            cursor: pointer;
            display: flex;
            align-items: center;
            gap: 0.75rem;
        }}
        .model-option:hover {{
            background-color: #f7fafc;
            border-color: #cbd5e0;
        }}
        .model-option input[type="radio"] {{
            cursor: pointer;
            margin: 0;
        }}
        .model-option label {{
            cursor: pointer;
            display: flex;
            align-items: center;
            gap: 0.75rem;
            margin: 0;
            flex: 1;
        }}
        .model-name {{
            font-weight: 600;
            color: #2d3748;
            font-size: 0.95rem;
            margin: 0;
        }}
        .provider-url {{
            font-size: 0.8rem;
            color: #718096;
            font-family: monospace;
            margin: 0;
        }}
        button {{
            width: 100%;
            padding: 0.75rem;
            background-color: #3182ce;
            color: white;
            border: none;
            border-radius: 6px;
            cursor: pointer;
            font-size: 1rem;
            font-weight: 500;
            transition: background-color 0.2s;
        }}
        button:hover {{
            background-color: #2c5282;
        }}
        .models-list::-webkit-scrollbar {{
            width: 8px;
        }}
        .models-list::-webkit-scrollbar-track {{
            background: #f1f1f1;
            border-radius: 4px;
        }}
        .models-list::-webkit-scrollbar-thumb {{
            background: #cbd5e0;
            border-radius: 4px;
        }}
        .models-list::-webkit-scrollbar-thumb:hover {{
            background: #a0aec0;
        }}
    </style>
</head>
<body>
    <div class="container">
        <h1>Set Default Model</h1>
        <form action="/set_default" method="post">
            <div class="filter-container">
                <input type="text" id="model-filter" placeholder="Filter models by name..." autocomplete="off">
            </div>
            <div class="models-list">
                {}
            </div>
</form>
<button id="refresh-btn" onclick="refreshModels()" style="margin-top: 1rem; width: auto; background-color: #48bb78;">Refresh Provider Models</button>
</div>
<script>
async function refreshModels() {{
const refreshBtn = document.getElementById('refresh-btn');
const originalText = refreshBtn.textContent;
refreshBtn.disabled = true;
refreshBtn.textContent = 'Refreshing...';
document.querySelectorAll('.model-option').forEach(function(opt) {{
opt.style.opacity = '0.5';
}});
try {{
const response = await fetch('/refresh_models', {{ method: 'POST' }});
if (response.ok) {{
window.location.reload();
}} else {{
const data = await response.json();
alert(data.error || 'Failed to refresh models');
refreshBtn.disabled = false;
refreshBtn.textContent = originalText;
document.querySelectorAll('.model-option').forEach(function(opt) {{
opt.style.opacity = '1';
}});
}}
}} catch (error) {{
console.error('Error refreshing models:', error);
alert('Error refreshing models');
refreshBtn.disabled = false;
refreshBtn.textContent = originalText;
document.querySelectorAll('.model-option').forEach(function(opt) {{
opt.style.opacity = '1';
}});
}}
}}

document.addEventListener('DOMContentLoaded', function() {{
            const filterInput = document.getElementById('model-filter');
            const modelOptions = document.querySelectorAll('.model-option');
            const radioButtons = document.querySelectorAll('input[type="radio"]');

            filterInput.addEventListener('input', function() {{
                const filterText = this.value.toLowerCase();
                modelOptions.forEach(function(option) {{
                    const modelName = option.querySelector('.model-name').textContent.toLowerCase();
                    if (modelName.includes(filterText)) {{
                        option.style.display = 'flex';
                    }} else {{
                        option.style.display = 'none';
                    }}
                }});
            }});

            // Handle radio button clicks - send request immediately
            radioButtons.forEach(function(radio) {{
                radio.addEventListener('change', async function() {{
                    const formData = new FormData();
                    formData.append('default_model', this.value);

                    // Show loading state
                    document.querySelectorAll('.model-option').forEach(function(opt) {{
                        opt.style.opacity = '0.5';
                    }});

                    try {{
                        const response = await fetch('/set_default', {{
                            method: 'POST',
                            headers: {{
                                'Content-Type': 'application/x-www-form-urlencoded'
                            }},
                            body: new URLSearchParams(formData)
                        }});

                        if (response.ok) {{
                            // Show success feedback
                            const successMsg = document.createElement('div');
                            successMsg.textContent = 'Default model updated successfully!';
                            successMsg.style.cssText = 'position: fixed; bottom: 20px; right: 20px; background: #48bb78; color: white; padding: 1rem 1.5rem; border-radius: 8px; box-shadow: 0 4px 6px rgba(0,0,0,0.1); z-index: 1000; animation: fadeIn 0.3s ease;';
                            document.body.appendChild(successMsg);

                            // Remove success message after 2 seconds
                            setTimeout(function() {{ successMsg.remove(); }}, 2000);
                        }} else {{
                            console.error('Failed to update default model');
                            // Show error feedback
                            const errorMsg = document.createElement('div');
                            errorMsg.textContent = 'Failed to update default model';
                            errorMsg.style.cssText = 'position: fixed; bottom: 20px; right: 20px; background: #f56565; color: white; padding: 1rem 1.5rem; border-radius: 8px; box-shadow: 0 4px 6px rgba(0,0,0,0.1); z-index: 1000; animation: fadeIn 0.3s ease;';
                            document.body.appendChild(errorMsg);

                            // Remove error message after 2 seconds
                            setTimeout(function() {{ errorMsg.remove(); }}, 2000);
                        }}
                    }} catch (error) {{
                        console.error('Error updating default model:', error);
                        // Show error feedback
                        const errorMsg = document.createElement('div');
                        errorMsg.textContent = 'Error updating default model';
                        errorMsg.style.cssText = 'position: fixed; bottom: 20px; right: 20px; background: #f56565; color: white; padding: 1rem 1.5rem; border-radius: 8px; box-shadow: 0 4px 6px rgba(0,0,0,0.1); z-index: 1000; animation: fadeIn 0.3s ease;';
                        document.body.appendChild(errorMsg);

                        // Remove error message after 2 seconds
                        setTimeout(function() {{ errorMsg.remove(); }}, 2000);
                    }} finally {{
                        // Restore opacity
                        document.querySelectorAll('.model-option').forEach(function(opt) {{
                            opt.style.opacity = '1';
                        }});
                    }}
                }});
            }});

            // Add fade-in animation
            const style = document.createElement('style');
            style.textContent = `
                @keyframes fadeIn {{
                    from {{ opacity: 0; transform: translateY(10px); }}
                    to {{ opacity: 1; transform: translateY(0); }}
                }}
            `;
            document.head.appendChild(style);
        }});
    </script>
</body>
</html>
"#,
        models_html
    );

    Html(html)
}

pub async fn set_default_post_handler(
    State(state): State<AppState>,
    Form(payload): Form<SetDefaultRequest>,
) -> impl IntoResponse {
    let load_balancer = &state.load_balancer;

    let new_default = if payload.default_model.is_empty() {
        None
    } else {
        Some(payload.default_model)
    };

    load_balancer.set_default_model(new_default).await;

    // Redirect back to GET page
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("Location", "/set_default")
        .body(Body::empty())
        .unwrap()
}

/// Translate Anthropic request to OpenAI format
fn anthropic_to_openai_request(anthropic: &AnthropicRequest) -> Value {
    let mut openai_messages = Vec::new();

    // Add system message if present
    if let Some(system) = &anthropic.system {
        let system_text = match system {
            AnthropicSystem::Text(text) => text.clone(),
            AnthropicSystem::Blocks(blocks) => {
                // Concatenate text blocks from system
                blocks
                    .iter()
                    .filter(|b| b.block_type == "text")
                    .map(|b| b.text.clone())
                    .collect::<Vec<_>>()
                    .join("\n\n")
            }
        };
        if !system_text.trim().is_empty() {
            openai_messages.push(json!({
                "role": "system",
                "content": system_text.trim()
            }));
        }
    }

    // Convert Anthropic messages to OpenAI format
    for msg in &anthropic.messages {
        let content = match &msg.content {
            AnthropicContent::Text(text) => text.clone(),
            AnthropicContent::Blocks(blocks) => {
                // Concatenate text blocks
                blocks
                    .iter()
                    .filter_map(|b| b.text.as_ref())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        };
        openai_messages.push(json!({
            "role": msg.role,
            "content": content
        }));
    }

    // Build OpenAI request
    let mut openai_request = json!({
        "model": anthropic.model,
        "messages": openai_messages
    });

    // Copy over any extra parameters (max_tokens, temperature, etc.)
    for (key, value) in &anthropic.extra_params {
        // Map Anthropic-specific parameter names to OpenAI equivalents if needed
        let openai_key = match key.as_str() {
            "max_tokens" => "max_tokens",
            "temperature" => "temperature",
            "top_p" => "top_p",
            "top_k" => "top_k",
            "stop_sequences" => "stop",
            "stream" => "stream",
            _ => key,
        };
        openai_request[openai_key] = value.clone();
    }

    // Convert tools if present
    if let Some(tools) = &anthropic.tools {
        let mut openai_tools = Vec::new();
        for tool in tools {
            if !tool.name.is_empty() {
                openai_tools.push(json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description.as_deref().unwrap_or(""),
                        "parameters": tool.input_schema.clone(),
                    },
                }));
            }
        }
        if !openai_tools.is_empty() {
            openai_request["tools"] = json!(openai_tools);
        }
    }

    // Convert tool choice if present
    if let Some(tool_choice) = &anthropic.tool_choice {
        let openai_tool_choice = match tool_choice.choice_type.as_str() {
            "auto" => json!("auto"),
            "any" => json!("auto"),
            "tool" => {
                if let Some(name) = &tool_choice.name {
                    json!({
                        "type": "function",
                        "function": {
                            "name": name,
                        },
                    })
                } else {
                    json!("auto")
                }
            }
            _ => json!("auto"),
        };
        openai_request["tool_choice"] = openai_tool_choice;
    }

    openai_request
}

/// Translate OpenAI response to Anthropic format
fn openai_to_anthropic_response(openai: &OpenAIResponse, model: &str) -> AnthropicResponse {
    let mut content_blocks = Vec::new();

    if let Some(choice) = openai.choices.first() {
        let message = &choice.message;

        // Add text content if present
        if let Some(text) = &message.content {
            if !text.is_empty() {
                content_blocks.push(AnthropicResponseContent::Text {
                    content_type: "text".to_string(),
                    text: text.clone(),
                });
            }
        }

        // Add tool calls if present
        for tool_call in &message.tool_calls {
            if tool_call.tool_type == "function" {
                // Parse arguments JSON
                let input = match serde_json::from_str::<Value>(&tool_call.function.arguments) {
                    Ok(v) => v,
                    Err(_) => json!({"raw_arguments": tool_call.function.arguments.clone()}),
                };

                content_blocks.push(AnthropicResponseContent::ToolUse {
                    content_type: "tool_use".to_string(),
                    id: tool_call.id.clone(),
                    name: tool_call.function.name.clone(),
                    input,
                });
            }
        }
    }

    // Ensure at least one content block
    if content_blocks.is_empty() {
        content_blocks.push(AnthropicResponseContent::Text {
            content_type: "text".to_string(),
            text: String::new(),
        });
    }

    // Map finish reason to stop reason
    let stop_reason = match openai.choices.first().map(|c| c.finish_reason.as_str()) {
        Some("stop") => "end_turn",
        Some("length") => "max_tokens",
        Some("tool_calls") => "tool_use",
        Some("function_call") => "tool_use",
        Some("content_filter") => "stop_sequence",
        Some(reason) => reason,
        None => "end_turn",
    };

    AnthropicResponse {
        id: openai.id.clone(),
        response_type: "message".to_string(),
        role: "assistant".to_string(),
        content: content_blocks,
        model: model.to_string(),
        stop_reason: stop_reason.to_string(),
        stop_sequence: None,
        usage: AnthropicUsage {
            input_tokens: openai.usage.prompt_tokens,
            output_tokens: openai.usage.completion_tokens,
        },
    }
}

/// Anthropic-compatible /v1/messages endpoint
/// Translates Anthropic requests to OpenAI format and responses back to Anthropic format
pub async fn anthropic_messages_handler(
    State(state): State<AppState>,
    Json(anthropic_request): Json<AnthropicRequest>,
) -> Response {
    let load_balancer = &state.load_balancer;
    let model_name = &anthropic_request.model;

    info!("Received Anthropic messages request for model: {}", model_name);

    // Check model support
    let selected_model = if model_name == "default" {
        load_balancer.get_default_model().await
    } else {
        Some(model_name.clone())
    };

    let selected_model = match selected_model {
        Some(m) => m,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "No default model found".to_string(),
                }),
            )
                .into_response();
        }
    };

    // Verify model exists in any provider, if not, use default model
    let final_model = if load_balancer.find_provider_for_model(&selected_model).is_none() {
        warn!(
            "Model '{}' not found in any provider, falling back to default model",
            selected_model
        );
        match load_balancer.get_default_model().await {
            Some(default_model) => {
                info!("Using default model: {}", default_model);
                default_model
            }
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: format!(
                            "Model '{}' not found and no default model configured",
                            selected_model
                        ),
                    }),
                )
                    .into_response();
            }
        }
    } else {
        selected_model
    };

    // Translate Anthropic request to OpenAI format
    let openai_request = anthropic_to_openai_request(&anthropic_request);

    // Check if streaming is requested
    let is_streaming = openai_request
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut last_error = None;
    let num_providers = load_balancer.get_config().providers.len();
    let known_provider_index = load_balancer.find_provider_for_model(&final_model);

    for _attempt in 0..num_providers {
        let provider_idx = load_balancer.get_current_provider_index();

        // Skip providers that don't support this model
        if let Some(known_idx) = known_provider_index {
            if provider_idx != known_idx {
                debug!(
                    "Skipping provider {} as it doesn't support model '{}'",
                    provider_idx, final_model
                );
                load_balancer.advance_provider_index();
                continue;
            }
        }

        let provider = match load_balancer.get_provider(provider_idx) {
            Some(p) => p,
            None => {
                last_error = Some(format!("Provider {} not found", provider_idx));
                continue;
            }
        };

        info!("Attempting provider {}: {}", provider_idx, provider.base_url);

        // Build the request body with provider-specific extra params
        let mut request_body = openai_request.clone();

        // Replace the model with the final model (either the requested model or default)
        request_body["model"] = json!(final_model);

        for (key, value) in &provider.extra_params {
            if key != "base_url" && key != "key" && key != "model_regex" {
                if let Some(value) = serde_json::to_value(value).ok() {
                    request_body[key] = value;
                }
            }
        }

        // Forward the request to the provider
        let url = format!("{}/chat/completions", provider.base_url);
        let client = load_balancer.get_http_client();
        let request_builder = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", provider.key))
            .json(&request_body);

        match request_builder.send().await {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    info!("Successfully forwarded request to provider {}", provider_idx);

                    if is_streaming {
                        // For streaming, we need to translate the stream on-the-fly
                        // This is more complex and would require a streaming transformer
                        // For now, we'll return the stream as-is (not fully Anthropic-compatible)
                        let mut response_builder = Response::builder()
                            .status(StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK));

                        if let Some(headers) = response_builder.headers_mut() {
                            for (name, value) in response.headers() {
                                if name != "content-length" && name != "transfer-encoding" {
                                    headers.insert(name, value.clone());
                                }
                            }
                        }

                        let stream = response.bytes_stream();
                        return response_builder
                            .body(Body::from_stream(stream))
                            .unwrap()
                            .into_response();
                    }

                    // Get the response body
                    let body_bytes = match response.bytes().await {
                        Ok(bytes) => bytes.to_vec(),
                        Err(e) => {
                            last_error = Some(format!(
                                "Failed to read response body from provider {}: {}",
                                provider_idx, e
                            ));
                            continue;
                        }
                    };

                    // Parse OpenAI response
                    let openai_response: OpenAIResponse = match serde_json::from_slice(&body_bytes)
                    {
                        Ok(resp) => resp,
                        Err(e) => {
                            error!("Failed to parse OpenAI response: {}", e);
                            last_error = Some(format!("Failed to parse provider response: {}", e));
                            continue;
                        }
                    };

                    // Translate to Anthropic format
                    let anthropic_response =
                        openai_to_anthropic_response(&openai_response, &final_model);

                    // Return Anthropic-formatted response
                    return Json(anthropic_response).into_response();
                } else {
                    let error_text = match response.text().await {
                        Ok(text) => text,
                        Err(_) => "Unknown error".to_string(),
                    };
                    last_error = Some(format!(
                        "Provider {} returned status {}: {}",
                        provider_idx, status, error_text
                    ));
                    error!(
                        "Provider {} failed with status {}: {}",
                        provider_idx, status, error_text
                    );
                }
            }
            Err(e) => {
                last_error = Some(format!("Request to provider {} failed: {:?}", provider_idx, e));
                error!("Provider {} failed with exception: {:?}", provider_idx, e);
            }
        }

        load_balancer.advance_provider_index();
    }

    // All providers failed
    let error_msg = last_error.unwrap_or_else(|| "Unknown error".to_string());
    error!("All providers failed. Last error: {}", error_msg);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: format!("All providers failed. Last error: {}", error_msg),
        }),
    )
        .into_response()
}