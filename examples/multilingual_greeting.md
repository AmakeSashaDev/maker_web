# Multilingual Greeting

A simple HTTP server that returns "Hello World!" in different languages depending on the first segment of the URL path.

**Example Features:**
- Handling various URL paths (`/en`, `/ru`, `/zh`, etc.)
- UTF-8 support (Cyrillic, Chinese, Arabic)
- Simple routing via `match`
- JSON responses with proper HTTP status codes

## Launch
```
cargo run --example multilingual_greeting
```

## Usage
- Get list of supported languages
  ```
  curl http://localhost:8080/
  # {"supported_lang": ["en", "zh", "es", "ar", "pt", "hi", "ru"]}
  ```
- Get greeting in specific language
  ```
  # English
  curl http://localhost:8080/api/en
  # {"lang": "en", "text": "Hello, world!"}
  
  # Russian
  curl http://localhost:8080/api/ru
  # {"lang": "ru", "text": "Привет, мир!"}
  
  # Chinese
  curl http://localhost:8080/api/zh
  # {"lang": "zh", "text": "你好世界！"}
  
  # Spanish
  curl http://localhost:8080/api/es
  # {"lang": "es", "text": "¡Hola Mundo!"}
  ```

## Errors  
- Incorrect language
  ```
  curl -i http://localhost:8080/api/fr
  # HTTP/1.1 404 Not Found
  # {"error": "Language not supported", "status": "Not Found"}
  ```