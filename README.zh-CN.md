<div align="center">
  <h1>maker_web</h1>
  <h3>安全优先、高性能、零分配的微服务HTTP服务器</h3>
</div>

[![下载量](https://img.shields.io/crates/d/maker_web?label=下载量)](https://crates.io/crates/maker_web)
[![版本](https://img.shields.io/crates/v/maker_web?label=版本)](https://crates.io/crates/maker_web)
[![文档](https://img.shields.io/badge/文档-docs.rs-blue)](https://docs.rs/maker_web/latest/maker_web/)
[![构建状态](https://github.com/AmakeSashaDev/maker_web/actions/workflows/ci.yml/badge.svg)](https://github.com/AmakeSashaDev/maker_web/actions)
[![GitHub](https://img.shields.io/badge/GitHub-主仓库-181717)](https://github.com/AmakeSashaDev/maker_web)

<div align="right">
    <a href="https://github.com/AmakeSashaDev/maker_web/blob/main/README.md">English version 🇺🇸</a> | 中文版 🇨🇳
</div>

# ✨ 特征

## 🔒 安全与防护
- **内置DoS/DDoS防护** - 默认启用，**无性能损耗**
- **请求、响应和连接的**限制与超时**均可完全配置**
- **自定义连接过滤** - 实现 [`ConnectionFilter`](https://docs.rs/maker_web/latest/maker_web/trait.ConnectionFilter.html) 特质，可在 **TCP 层** 拒绝不需要的连接

## ⚡ 性能与内存

- **零分配** - 服务器启动后不进行任何内存分配
- **每个连接预分配内存** - 线性透明扩展

## 🌐 协议与管理

- **完整的 HTTP 堆栈** - `HTTP/1.1`, `HTTP/1.0`, [`HTTP/0.9+`
  ](https://docs.rs/maker_web/latest/maker_web/limits/struct.Http09Limits.html) 带 keep-alive 功能
- **无需指定即可自动检测协议**
- **在请求之间存储数据** - 能够在单个连接中使用 [`ConnectionData`](https://docs.rs/maker_web/latest/maker_web/trait.ConnectionData.html) 特性在请求之间存储数据

## 🏭 生产就绪

- **优雅的性能降级** - 过载时自动返回 503 响应
- [**自定义错误格式**](https://docs.rs/maker_web/latest/maker_web/limits/struct.ServerLimits.html#structfield.json_errors) - 结构化的 JSON（带有代码/描述）或纯 HTTP 响应
- **资源保护** - 自动关闭超出设定限制的连接

# 🎯 用例

- **高吞吐量微服务** - 可针对特定工作负载进行配置
- **资源受限环境** - 可预测的内存使用情况 
- **内部 API** - 安全至上的默认设置
- **性能关键型应用** - 零分配设计
- **旧系统集成支持** - 兼容 `HTTP/1.0` 与 `HTTP/0.9` 协议

# 🌐 不仅仅是代码

所有未写入文档的内容——实时统计数据、深度细节和非正式计划——我都收集在一个[单独的网站](https://amakesashadev.github.io/maker_web/)上。我努力保持这个空间的内容更新及时且有意义。

**如果此网站无法正常运行 :**

您也可以在本地运行该网站，无需安装。只需在浏览器中打开文件 [`docs/index.html`](/docs/index.html) 即可。

# 🚀 快速入门

## 1. 安装

将 `maker_web` 和 [`tokio`](https://crates.io/crates/tokio) 添加到您的 `Cargo.toml` 文件中:
```bash
cargo add maker_web tokio --features tokio/full
```
或者手动:
```toml
[dependencies]
maker_web = "0.1"
tokio = { version = "1", features = ["full"] }
```

## 2. 使用示例
```rust
use maker_web::{Handled, Handler, Request, Response, Server, StatusCode};
use tokio::net::TcpListener;

struct MyHandler;

impl Handler for MyHandler {
    async fn handle(&self, _: &mut (), req: &Request, resp: &mut Response) -> Handled {
        match req.url().path_segments_str() {
            ["api", user, "name"] => {
                resp.status(StatusCode::Ok).body(user)
            }
            ["api", user, "name", "len"] => {
                resp.status(StatusCode::Ok).body(user.len())
            }
            ["api", "echo", text] => {
                resp.status(StatusCode::Ok).body(text)
            }
            _ => resp.status(StatusCode::NotFound).body("qwe"),
        }
    }
}

#[tokio::main]
async fn main() {
    Server::builder()
        .listener(TcpListener::bind("127.0.0.1:8080").await.unwrap())
        .handler(MyHandler)
        .build()
        .launch()
        .await;
}
```

# 📖示例

详细的使用示例可以在[示例目录](https://github.com/AmakeSashaDev/maker_web/blob/main/examples)中找到

# 📊性能基准测试

性能对比数据可在[基准测试目录](https://github.com/AmakeSashaDev/maker_web/tree/main/benches)中找到。

# 📄 许可

`maker_web` 以以下许可之一分发，您可以选择其中之一：
* [MIT 许可](https://github.com/AmakeSashaDev/maker_web/blob/main/LICENSE-MIT)
* [Apache 2.0 许可](https://github.com/AmakeSashaDev/maker_web/blob/main/LICENSE-APACHE)
