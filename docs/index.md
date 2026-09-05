# 无限画布文档索引

## 项目介绍

- [小陈的画布：产品需求与验收标准（实施与验收共同依据）](overview/product-requirements.md)
- [快速开始](overview/quick-start.md)
- [功能介绍](overview/features.md)
- [Docker 部署](overview/docker.md)
- [第三方 GitHub 提示词仓库](overview/third-party-prompt-repositories.md)

## 操作手册

- [画布节点操作手册](canvas/canvas-node-manual.md)
- [画布快捷键](canvas/canvas-shortcuts.md)

## 开发文档

- [本地开发](backend/local-development.md)
- [接口响应约定](backend/api-response.md)
- [系统配置数据结构](backend/system-settings.md)
- [后端数据库说明](backend/backend-database.md)
- [画布数据结构](backend/canvas-data-structure.md)
- [本机 Agent 适配层与 CLI](development/local-agent-adapter.md)
- [本机 Agent 适配层总装交接](development/local-agent-adapter-handoff.md)

## 商务合作

- [开源协议](business/license.md)
- [商务合作](business/business.md)

## 赞助支持

- [打赏支持](support/donate.md)

## 项目进度

- [修复、Rust 迁移与正式安装验收](progress/canvas-rust-repair.md)
- [画布交互与素材优化](progress/canvas-ui-optimization.md)
- [版本历史、数据与请求优化](progress/canvas-data-optimization.md)
- [待测试](progress/pending-test.md)
- [TODO](progress/todo.md)
- [macOS 桌面导演台验收矩阵](development/macos-director-acceptance-matrix.md)

## 说明

- 当前个人桌面版不提供 App 账号与云同步入口，旧记录保留；上游账号相关历史文档不代表本分支现有功能。
- macOS 桌面版使用独立本机身份把画布保存到应用数据库，供 WebView 和受控本机 Agent Bridge 共同操作。
- 本地直连模式下，AI API Key 保存在浏览器本地，并由前端直接请求 OpenAI 兼容接口。
