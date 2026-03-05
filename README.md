# LLM Load Balancer - Rust Implementation

A high-performance, multi-threaded async LLM load balancer written in Rust with a GUI interface for model selection.

## Features

- **High Performance**: Built with Rust and Tokio async runtime for maximum throughput
- **OpenAI Compatible**: Provides minimal OpenAI-compatible API endpoints
- **Model Discovery**: Automatically fetches available models from all providers at startup
- **Exact Model Matching**: Uses hash table for efficient exact model name matching (no regex)
- **Load Balancing**: Round-robin load balancing across providers with failover
- **GUI Interface**: Interactive model selector for choosing default model
- **Default Model Support**: Request with `model: "default"` uses GUI-selected model
- **Multi-Provider**: Supports multiple LLM providers simultaneously

## Requirements

- Rust 1.70 or higher
- Cargo

## Building

```bash
cd llm_load_balancer_rs
cargo build --release
```

## Configuration

Create a YAML configuration file (e.g., `config.yaml`):

```yaml
port: 7128
providers:
  - base_url: https://openrouter.ai/api/v1
    key: your-api-key-here
    reasoning:
      max_tokens: 2000
  - base_url: https://api-inference.modelscope.cn/v1
    key: your-api-key-here
    enable_thinking: true
  - base_url: https://integrate.api.nvidia.com/v1
    key: your-api-key-here
    max_tokens: 65536
```

### Configuration Fields

- `port`: Port number for the load balancer to listen on
- `providers`: List of provider configurations
  - `base_url`: Base URL of the provider's API
  - `key`: API key for authentication
  - Additional fields are passed through to the provider as extra parameters

## Running

```bash
# Run with default config (test.yaml)
cargo run --release

# Run with custom config
cargo run --release -- path/to/config.yaml
```

## API Endpoints

### POST /v1/chat/completions

OpenAI-compatible chat completion endpoint.

Request format:
```json
{
  "model": "model-name-or-default",
  "messages": [
    {
      "role": "user",
      "content": "Hello, how are you?"
    }
  ]
}
```

- Use `"default"` as the model name to use the GUI-selected default model
- Use an exact model name to route to the specific provider
- If model not found, falls back to round-robin load balancing

### GET /models

Get list of all available models from all providers.

Response format:
```json
{
  "data": [
    {
      "provider_index": 0,
      "base_url": "https://openrouter.ai/api/v1",
      "models": [
        {
          "id": "google/gemini-pro",
          "object": "model",
          "created": 1234567890,
          "owned_by": "Google"
        }
      ]
    }
  ],
  "object": "list",
  "total_providers": 1
}
```

### GET /health

Health check endpoint.

## GUI Features

The GUI window allows you to:

- View all available models from all providers
- Search and filter models by name
- Select a default model (used when request specifies `model: "default"`)
- Clear the default model (falls back to round-robin)
- See provider information for each model

## How It Works

1. **Startup**: On startup, the load balancer fetches the `/models` endpoint from each provider
2. **Model Mapping**: Creates a hash table mapping exact model names to provider indices
3. **Request Handling**:
   - If `model` is `"default"`: Uses GUI-selected model or falls back to round-robin
   - If `model` is an exact name: Routes to the specific provider
   - If `model` is not found: Falls back to round-robin load balancing
4. **Load Balancing**: Uses round-robin algorithm with atomic counter for thread-safe provider selection
5. **Failover**: If a provider fails, automatically tries the next provider

## Architecture

- **Tokio**: Async runtime for high concurrency
- **Axum**: Web framework for HTTP server
- **dashmap**: Concurrent hash map for thread-safe model-to-provider mapping
- **egui/eframe**: GUI framework
- **reqwest**: Async HTTP client

## Performance Improvements Over Python Version

- Compiled to native code (no interpreter overhead)
- Zero-cost abstractions with Rust
- Efficient async I/O with Tokio
- Thread-safe data structures with dashmap
- Better memory management (no GC pauses)

## License

MIT
