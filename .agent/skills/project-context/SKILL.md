---
name: project-context
description: Project-specific context, rules, and commands automatically migrated from all CLAUDE.md files in the repo. Use this to understand the project structure and conventions.
triggers:
  - project context
  - project rules
  - explain project
---

# Root Context (CLAUDE.md)

# back-end_python-service_fetch-finance-data Development Guidelines

Auto-generated from all feature plans. Last updated: 2026-02-10

## Active Technologies
- Go 1.21+ + Gin (HTTP 框架), clickhouse-go v2 (ClickHouse 驱动), zap (日志), viper (配置), swaggo (Swagger 文档) (002-back-end_go-service_finance-data-api)
- ClickHouse (`stock` 数据库，`kline_ohlcv` + `adjust_factor` 表) (002-back-end_go-service_finance-data-api)
- Rust 1.75+ (edition 2021) + wasm-bindgen 0.2, serde 1.0, serde-wasm-bindgen 0.6 (004-common_rust_indicators-computation)
- N/A（纯计算库，无持久化） (004-common_rust_indicators-computation)
- Python 3.11 + futu-api (>=9.4), akshare, clickhouse-driver (>=0.2.9), python-dotenv, pydantic (001-back-end_python-service_fetch-finance-data)

## Project Structure

```text
src/
tests/
```

## Commands

cd src [ONLY COMMANDS FOR ACTIVE TECHNOLOGIES][ONLY COMMANDS FOR ACTIVE TECHNOLOGIES] pytest [ONLY COMMANDS FOR ACTIVE TECHNOLOGIES][ONLY COMMANDS FOR ACTIVE TECHNOLOGIES] ruff check .

## Code Style

Python 3.11: Follow standard conventions

## Recent Changes

- 001-back-end_python-service_fetch-finance-data: Added Python 3.11 + futu-api (>=9.4), akshare, clickhouse-driver (>=0.2.9), python-dotenv, pydantic

<!-- MANUAL ADDITIONS START -->
<!-- MANUAL ADDITIONS END -->
