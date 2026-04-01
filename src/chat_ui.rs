use crate::api::AppState;
use axum::{
    extract::State,
    response::{Html, IntoResponse},
};

pub async fn chat_ui_handler(State(state): State<AppState>) -> impl IntoResponse {
    let load_balancer = &state.load_balancer;
    let default_model = load_balancer.get_default_model().await;
    let default_model_name = default_model.unwrap_or_else(|| "default".to_string());
    let html = generate_chat_html(&default_model_name);
    Html(html)
}

fn generate_chat_html(default_model_name: &str) -> String {
    format!(r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>LLM Chat - Load Balancer</title>
    <style>
        * {{ box-sizing: border-box; margin: 0; padding: 0; }}
        :root {{
            --bg-primary: #212121;
            --bg-secondary: #2f2f2f;
            --bg-tertiary: #424242;
            --text-primary: #ececec;
            --text-secondary: #b4b4b4;
            --accent: #10a37f;
            --accent-hover: #1a7f64;
            --border: #424242;
            --error: #ef4444;
        }}
        body {{
            font-family: 'Söhne', 'ui-sans-serif', 'system-ui', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background-color: var(--bg-primary);
            color: var(--text-primary);
            height: 100vh;
            display: flex;
            flex-direction: column;
            overflow: hidden;
        }}
        .header {{
            display: flex;
            align-items: center;
            justify-content: space-between;
            padding: 12px 16px;
            border-bottom: 1px solid var(--border);
            background-color: var(--bg-secondary);
        }}
        .header h1 {{ font-size: 1rem; font-weight: 600; }}
        .header-links {{ display: flex; gap: 12px; }}
        .header-links a {{ color: var(--text-secondary); text-decoration: none; font-size: 0.875rem; }}
        .header-links a:hover {{ color: var(--text-primary); }}
        .model-info {{
            display: flex;
            align-items: center;
            gap: 8px;
            padding: 8px 16px;
            background-color: var(--bg-secondary);
            border-bottom: 1px solid var(--border);
            font-size: 0.875rem;
        }}
        .model-badge {{
            background-color: var(--accent);
            color: white;
            padding: 4px 10px;
            border-radius: 12px;
            font-size: 0.75rem;
            font-weight: 500;
        }}
        .chat-container {{
            flex: 1;
            overflow-y: auto;
            padding: 20px;
            scroll-behavior: smooth;
        }}
        .messages {{
            max-width: 800px;
            margin: 0 auto;
            display: flex;
            flex-direction: column;
            gap: 24px;
        }}
        .welcome {{ text-align: center; padding: 60px 20px; color: var(--text-secondary); }}
        .welcome h2 {{ font-size: 1.5rem; margin-bottom: 8px; color: var(--text-primary); }}
        .welcome p {{ font-size: 1rem; }}
        .message {{ display: flex; gap: 16px; animation: fadeIn 0.3s ease; }}
        @keyframes fadeIn {{
            from {{ opacity: 0; transform: translateY(10px); }}
            to {{ opacity: 1; transform: translateY(0); }}
        }}
        .message-avatar {{
            width: 36px;
            height: 36px;
            border-radius: 4px;
            display: flex;
            align-items: center;
            justify-content: center;
            font-weight: 600;
            font-size: 0.875rem;
            flex-shrink: 0;
        }}
        .message.user .message-avatar {{ background-color: #5436da; }}
        .message.assistant .message-avatar {{ background-color: var(--accent); }}
        .message-content {{ flex: 1; min-width: 0; }}
        .message-header {{ display: flex; align-items: center; gap: 8px; margin-bottom: 8px; flex-wrap: wrap; }}
        .message-role {{ font-weight: 600; font-size: 0.875rem; }}
        .message-meta {{ font-size: 0.75rem; color: var(--text-secondary); display: flex; gap: 12px; flex-wrap: wrap; }}
        .tps-badge {{ background-color: rgba(16, 163, 127, 0.2); color: var(--accent); padding: 2px 8px; border-radius: 4px; font-weight: 500; }}
        .model-name-badge {{ background-color: rgba(84, 54, 218, 0.2); color: #a78bfa; padding: 2px 8px; border-radius: 4px; }}
        .message-text {{ font-size: 0.9375rem; line-height: 1.6; white-space: pre-wrap; word-wrap: break-word; }}
        .reasoning-content {{
            background-color: rgba(168, 85, 247, 0.1);
            border-left: 3px solid rgba(168, 85, 247, 0.5);
            padding: 12px;
            margin-bottom: 12px;
            border-radius: 0 4px 4px 0;
            font-size: 0.875rem;
            line-height: 1.6;
            color: #a78bfa;
            font-style: italic;
        }}
        .reasoning-toggle {{
            display: flex;
            align-items: center;
            gap: 8px;
            font-size: 0.75rem;
            color: var(--text-secondary);
            margin-bottom: 8px;
            cursor: pointer;
            user-select: none;
        }}
        .reasoning-toggle:hover {{
            color: var(--text-primary);
        }}
        .reasoning-toggle svg {{
            width: 14px;
            height: 14px;
            transition: transform 0.2s;
        }}
        .reasoning-toggle.expanded svg {{
            transform: rotate(180deg);
        }}
        .message-images {{ display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 12px; }}
        .message-images img {{ max-width: 200px; max-height: 200px; border-radius: 8px; object-fit: cover; cursor: pointer; }}
        .input-container {{ padding: 16px; background-color: var(--bg-primary); border-top: 1px solid var(--border); }}
        .input-wrapper {{ max-width: 800px; margin: 0 auto; }}
        .input-box {{ display: flex; flex-direction: column; background-color: var(--bg-tertiary); border: 1px solid var(--border); border-radius: 12px; overflow: hidden; }}
        .image-preview-container {{ display: none; flex-wrap: wrap; gap: 8px; padding: 12px; border-bottom: 1px solid var(--border); }}
        .image-preview-container.has-images {{ display: flex; }}
        .image-preview {{ position: relative; width: 80px; height: 80px; }}
        .image-preview img {{ width: 100%; height: 100%; object-fit: cover; border-radius: 8px; }}
        .image-preview .remove-image {{
            position: absolute;
            top: -6px;
            right: -6px;
            width: 20px;
            height: 20px;
            background-color: var(--error);
            color: white;
            border: none;
            border-radius: 50%;
            cursor: pointer;
            font-size: 12px;
            display: flex;
            align-items: center;
            justify-content: center;
        }}
        .input-row {{ display: flex; align-items: flex-end; }}
        textarea {{
            flex: 1;
            background: transparent;
            border: none;
            color: var(--text-primary);
            font-size: 1rem;
            padding: 12px;
            resize: none;
            outline: none;
            font-family: inherit;
            min-height: 24px;
            max-height: 200px;
        }}
        textarea::placeholder {{ color: var(--text-secondary); }}
        .input-actions {{ display: flex; padding: 12px; gap: 8px; }}
        .action-btn {{
            background: transparent;
            border: none;
            color: var(--text-secondary);
            cursor: pointer;
            padding: 8px;
            border-radius: 8px;
            display: flex;
            align-items: center;
            justify-content: center;
            transition: background-color 0.2s;
        }}
        .action-btn:hover {{ background-color: rgba(255, 255, 255, 0.1); }}
        .action-btn.send-btn {{ background-color: var(--accent); color: white; }}
        .action-btn.send-btn:hover {{ background-color: var(--accent-hover); }}
        .action-btn.send-btn:disabled {{ background-color: var(--border); cursor: not-allowed; }}
        .action-btn svg {{ width: 20px; height: 20px; }}
        #image-input {{ display: none; }}
        .loading {{ display: flex; gap: 4px; padding: 12px; }}
        .loading-dot {{ width: 8px; height: 8px; background-color: var(--text-secondary); border-radius: 50%; animation: bounce 1.4s infinite ease-in-out both; }}
        .loading-dot:nth-child(1) {{ animation-delay: -0.32s; }}
        .loading-dot:nth-child(2) {{ animation-delay: -0.16s; }}
        @keyframes bounce {{
            0%, 80%, 100% {{ transform: scale(0); }}
            40% {{ transform: scale(1); }}
        }}
        .error-message {{ color: var(--error); padding: 12px; text-align: center; }}
        ::-webkit-scrollbar {{ width: 8px; }}
        ::-webkit-scrollbar-track {{ background: transparent; }}
        ::-webkit-scrollbar-thumb {{ background: var(--border); border-radius: 4px; }}
        ::-webkit-scrollbar-thumb:hover {{ background: var(--text-secondary); }}
    </style>
</head>
<body>
    <div class="header">
        <h1>🤖 LLM Load Balancer Chat</h1>
        <div class="header-links">
            <a href="/set_default">Settings</a>
            <a href="/v1/models">Models API</a>
        </div>
    </div>
    <div class="model-info">
        <span>Current Model:</span>
        <span class="model-badge" id="current-model">{0}</span>
    </div>
    <div class="chat-container" id="chat-container">
        <div class="messages" id="messages">
            <div class="welcome">
                <h2>Welcome to LLM Chat</h2>
                <p>Send a message to start chatting with the AI model. You can also upload images.</p>
            </div>
        </div>
    </div>
    <div class="input-container">
        <div class="input-wrapper">
            <div class="input-box">
                <div class="image-preview-container" id="image-preview-container"></div>
                <div class="input-row">
                    <textarea id="message-input" placeholder="Type your message..." rows="1"></textarea>
                    <div class="input-actions">
                        <input type="file" id="image-input" accept="image/*" multiple>
                        <button class="action-btn" id="upload-btn" title="Upload image">
                            <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z" />
                            </svg>
                        </button>
                        <button class="action-btn send-btn" id="send-btn" title="Send message">
                            <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 19l9 2-9-18-9 18 9-2zm0 0v-8" />
                            </svg>
                        </button>
                    </div>
                </div>
            </div>
        </div>
    </div>
    <script>
        const messagesContainer = document.getElementById('messages');
        const chatContainer = document.getElementById('chat-container');
        const messageInput = document.getElementById('message-input');
        const sendBtn = document.getElementById('send-btn');
        const uploadBtn = document.getElementById('upload-btn');
        const imageInput = document.getElementById('image-input');
        const imagePreviewContainer = document.getElementById('image-preview-container');
        const currentModelBadge = document.getElementById('current-model');
        
        let conversationHistory = [];
        let selectedImages = [];
        let isLoading = false;
        let messageStartTime = 0;
        let inputTokens = 0;
        
        // Auto-resize textarea
        messageInput.addEventListener('input', function() {{
            this.style.height = 'auto';
            this.style.height = Math.min(this.scrollHeight, 200) + 'px';
        }});
        
        // Handle Enter key
        messageInput.addEventListener('keydown', function(e) {{
            if (e.key === 'Enter' && !e.shiftKey) {{
                e.preventDefault();
                sendMessage();
            }}
        }});
        
        // Upload button click
        uploadBtn.addEventListener('click', () => imageInput.click());
        
        // Handle image selection
        imageInput.addEventListener('change', function(e) {{
            const files = Array.from(e.target.files);
            files.forEach(file => {{
                if (file.type.startsWith('image/')) {{
                    const reader = new FileReader();
                    reader.onload = (e) => {{
                        selectedImages.push(e.target.result);
                        updateImagePreviews();
                    }};
                    reader.readAsDataURL(file);
                }}
            }});
            this.value = '';
        }});
        
        function updateImagePreviews() {{
            imagePreviewContainer.innerHTML = '';
            selectedImages.forEach((img, index) => {{
                const preview = document.createElement('div');
                preview.className = 'image-preview';
                preview.innerHTML = `
                    <img src="${{img}}" alt="Preview">
                    <button class="remove-image" onclick="removeImage(${{index}})">×</button>
                `;
                imagePreviewContainer.appendChild(preview);
            }});
            imagePreviewContainer.classList.toggle('has-images', selectedImages.length > 0);
        }}
        
        window.removeImage = function(index) {{
            selectedImages.splice(index, 1);
            updateImagePreviews();
        }};
        
        sendBtn.addEventListener('click', sendMessage);
        
        async function sendMessage() {{
            const content = messageInput.value.trim();
            if ((!content && selectedImages.length === 0) || isLoading) return;
            
            // Remove welcome message
            const welcome = messagesContainer.querySelector('.welcome');
            if (welcome) welcome.remove();
            
            // Build user message
            const userMessage = {{ role: 'user', content: content }};
            if (selectedImages.length > 0) {{
                userMessage.images = [...selectedImages];
            }}
            
            // Add to history
            conversationHistory.push(userMessage);
            
            // Display user message
            appendMessage('user', content, selectedImages);
            
            // Clear input
            messageInput.value = '';
            messageInput.style.height = 'auto';
            selectedImages = [];
            updateImagePreviews();
            
            // Prepare API request
            const messages = conversationHistory.map(msg => {{
                if (msg.images && msg.images.length > 0) {{
                    return {{
                        role: msg.role,
                        content: [
                            {{ type: 'text', text: msg.content || '' }},
                            ...msg.images.map(img => ({{
                                type: 'image_url',
                                image_url: {{ url: img }}
                            }}))
                        ]
                    }};
                }}
                return {{ role: msg.role, content: msg.content }};
            }});
            
            const requestBody = {{
                model: 'default',
                messages: messages,
                stream: true
            }};
            
            // Show loading
            isLoading = true;
            sendBtn.disabled = true;
            const loadingEl = document.createElement('div');
            loadingEl.className = 'message assistant';
            loadingEl.innerHTML = `
                <div class="message-avatar">AI</div>
                <div class="message-content">
                    <div class="loading">
                        <div class="loading-dot"></div>
                        <div class="loading-dot"></div>
                        <div class="loading-dot"></div>
                    </div>
                </div>
            `;
            messagesContainer.appendChild(loadingEl);
            scrollToBottom();
            
            messageStartTime = Date.now();
            inputTokens = estimateTokenCount(content);
            
            try {{
                const response = await fetch('/v1/chat/completions', {{
                    method: 'POST',
                    headers: {{ 'Content-Type': 'application/json' }},
                    body: JSON.stringify(requestBody)
                }});
                
                if (!response.ok) {{
                    throw new Error(`HTTP error! status: ${{response.status}}`);
                }}
                
                // Remove loading
                loadingEl.remove();
                
                // Create assistant message element with reasoning support
                const assistantEl = document.createElement('div');
                assistantEl.className = 'message assistant';
                assistantEl.innerHTML = `
                    <div class="message-avatar">AI</div>
                    <div class="message-content">
                        <div class="message-header">
                            <span class="message-role">Assistant</span>
                            <div class="message-meta">
                                <span class="model-name-badge" id="model-name"></span>
                                <span class="tps-badge" id="tps-info"></span>
                            </div>
                        </div>
                        <div class="reasoning-wrapper" id="reasoning-wrapper" style="display: none;">
                            <div class="reasoning-toggle" id="reasoning-toggle">
                                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
                                </svg>
                                <span>View reasoning</span>
                            </div>
                            <div class="reasoning-content" id="reasoning-content" style="display: none;"></div>
                        </div>
                        <div class="message-text" id="assistant-text"></div>
                    </div>
                `;
                messagesContainer.appendChild(assistantEl);
                
                const textEl = document.getElementById('assistant-text');
                const modelEl = document.getElementById('model-name');
                const tpsEl = document.getElementById('tps-info');
                const reasoningWrapper = document.getElementById('reasoning-wrapper');
                const reasoningToggle = document.getElementById('reasoning-toggle');
                const reasoningContent = document.getElementById('reasoning-content');
                
                let fullContent = '';
                let fullReasoning = '';
                let hasReasoning = false;
                let modelName = '';
                const reader = response.body.getReader();
                const decoder = new TextDecoder();
                
                let reasoningToggleAttached = false;
                
                while (true) {{
                    const {{ done, value }} = await reader.read();
                    if (done) break;
                    
                    const chunk = decoder.decode(value);
                    const lines = chunk.split('\n');
                    
                    for (const line of lines) {{
                        if (line.startsWith('data: ')) {{
                            const data = line.slice(6);
                            if (data === '[DONE]') continue;
                            
                            try {{
                                const json = JSON.parse(data);
                                
                                // Capture model name
                                if (json.model && !modelName) {{
                                    modelName = json.model;
                                    modelEl.textContent = modelName;
                                }}
                                
                                // Get content delta
                                const delta = json.choices?.[0]?.delta;
                                
                                // Handle reasoning content
                                if (delta?.reasoning_content) {{
                                    if (!hasReasoning) {{
                                        // First time seeing reasoning - show the toggle
                                        hasReasoning = true;
                                        reasoningWrapper.style.display = 'flex';
                                        if (!reasoningToggleAttached) {{
                                            reasoningToggle.addEventListener('click', function() {{
                                                const isExpanded = reasoningToggle.classList.toggle('expanded');
                                                reasoningContent.style.display = isExpanded ? 'block' : 'none';
                                            }});
                                            reasoningToggleAttached = true;
                                        }}
                                    }}
                                    fullReasoning += delta.reasoning_content;
                                    reasoningContent.textContent = fullReasoning;
                                }}
                                
                                // Handle regular content
                                if (delta?.content) {{
                                    fullContent += delta.content;
                                    textEl.textContent = fullContent;
                                }}
                            }} catch (e) {{
                                // Ignore parse errors for incomplete chunks
                            }}
                        }}
                    }}
                }}
                
                // Calculate TPS
                const elapsedSeconds = (Date.now() - messageStartTime) / 1000;
                const outputTokens = estimateTokenCount(fullContent);
                const tps = outputTokens > 0 ? (outputTokens / elapsedSeconds).toFixed(1) : '0';
                tpsEl.textContent = `${{outputTokens}} tokens · ${{tps}} TPS`;
                
                // Add to history
                conversationHistory.push({{ role: 'assistant', content: fullContent }});
                
                // Clean up IDs
                textEl.removeAttribute('id');
                modelEl.removeAttribute('id');
                tpsEl.removeAttribute('id');
                if (reasoningWrapper) reasoningWrapper.removeAttribute('id');
                if (reasoningToggle) reasoningToggle.removeAttribute('id');
                if (reasoningContent) reasoningContent.removeAttribute('id');
                
            }} catch (error) {{
                loadingEl.remove();
                const errorEl = document.createElement('div');
                errorEl.className = 'error-message';
                errorEl.textContent = `Error: ${{error.message}}`;
                messagesContainer.appendChild(errorEl);
                setTimeout(() => errorEl.remove(), 5000);
                
                // Remove failed message from history
                conversationHistory.pop();
            }} finally {{
                isLoading = false;
                sendBtn.disabled = false;
            }}
        }}
        
        function appendMessage(role, content, images = []) {{
            const el = document.createElement('div');
            el.className = `message ${{role}}`;
            
            let imagesHtml = '';
            if (images.length > 0) {{
                imagesHtml = `<div class="message-images">${{images.map(img => `<img src="${{img}}" onclick="window.open('${{img}}')">`).join('')}}</div>`;
            }}
            
            el.innerHTML = `
                <div class="message-avatar">${{role === 'user' ? 'U' : 'AI'}}</div>
                <div class="message-content">
                    <div class="message-header">
                        <span class="message-role">${{role === 'user' ? 'You' : 'Assistant'}}</span>
                    </div>
                    ${{imagesHtml}}
                    <div class="message-text">${{escapeHtml(content)}}</div>
                </div>
            `;
            messagesContainer.appendChild(el);
            scrollToBottom();
        }}
        
        function scrollToBottom() {{
            chatContainer.scrollTop = chatContainer.scrollHeight;
        }}
        
        function escapeHtml(text) {{
            const div = document.createElement('div');
            div.textContent = text;
            return div.innerHTML;
        }}
        
        function estimateTokenCount(text) {{
            // Rough estimation: ~4 characters per token for English
            return Math.ceil(text.length / 4);
        }}
    </script>
</body>
</html>"##, default_model_name)
}