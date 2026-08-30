---
title: TODO
description: 当前项目后续值得处理的事项
---

# TODO

本文档用来记录当前项目后续比较值得处理的事项。

- 统一操作核心合并时，用其 mutation/result 协议替换画布共编 adapter 的临时执行边界；保留现有 `CanvasCollaborationState`、节点人工锁和 revision guard，不复制核心 reducer。
- 总装后补一条真实零付费 Agent 多动作验收，覆盖执行中人工修改、锁定节点和冲突后的显式重试。
