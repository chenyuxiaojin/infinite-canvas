# 素材地址消费者与归还契约

本表记录当前源码契约；正式 App 交互和内存验收由主整合任务完成。稳定 storageKey、原文件和历史快照保留，仅释放临时显示地址。

| 消费者 | 当前实际路径 | 归还边界 |
| --- | --- | --- |
| 本机登记原图节点/全屏 | use-canvas-image-source → canvas-local-image | 原租用机制、两路读取；最后显示消费者归还 |
| 浏览器节点/全屏/音视频 | use-canvas-image-source → canvas-media-lease | 共享租用；最后显示或项目租用归还；迟到加载不分配地址 |
| 裁剪/分图/截帧与画布上传 | canvas-client-page → CanvasMediaScope | 项目作用域持有原图；离开项目或恢复版本重挂载关闭；异步迟到返回拒绝 |
| 全局素材库启动 | use-asset-store → stableAssetMedia | 只载稳定引用，不再遍历解析全部 Blob；旧无键内联数据保留原样 |
| 素材库卡片/详情/播放器 | assets/page → useStoredMediaSource | 卡片接近视口加载，离开归还；详情独立租用至关闭；下载读取原 Blob |
| 素材选择/拖放 | asset-picker-modal、canvas-side-panel | 卡片独立租用；传稳定键，目标项目持有自己的租用 |
| 素材编辑弹窗 | asset-form-modal | 预览租用至关闭；上传不保留无消费者 URL；保存稳定键 |
| 画布 ZIP 与素材包 | canvas-export、canvas-import、asset-transfer | 导出读取持久 Blob；导入 setBlob(..., false) 不产生显示地址 |
| Agent 图片 | canvas-media、imageToDataUrl | 原 Blob → data URL，仅请求作用域持有；旧远端回退地址临时 adopt 并 finally 归还 |
| 图片生成参考 | services/api/image | 公网地址选择不创建本地 URL；文件转换读取原 Blob |
| 视频/音频参考 | services/api/video、audio | 文件转换读取原 Blob；公网判断不分配本地 URL；元素参考保留原有 URL 协议，由工作台租用保护 |
| 图像/视频工作台历史与上传 | useMediaScope → CanvasMediaScope | 页面作用域保护编辑和历史引用，卸载归还；视频结果播放器独立租用 |
| 创作工作流参考 | creative-workflow-workspace → useMediaScope | 工作流实例卸载归还；插入素材先获得实例租用 |
| 本机/保护下载生成结果 | canvas-local-task、api/audio、api/video | 原 Blob 持久化后返回稳定键，不保留显示 URL；画布/播放器按需接管 |
| 旧云存储迁移 | storage-migration.ts | 当前 web/src 无引用入口，未执行；旧 URL 兼容 API 仅在文档退出保底释放，不宣称此休眠路径已迁移 |
| 版本历史 | canvas-history/store/Rust history | 只保存稳定引用；desktop 的仅当前引用清理停止删除原 Blob，避免损坏旧快照；成功恢复更新 restoredRevisions 并重挂载画布 |

已迁移路径的闲置显示地址为零；原字节读取并发上限为 2。使用中的页面原图可超过固定字节数，页面作用域为了编辑/历史保护会持有已加载原图直到卸载，这不是全应用内存硬上限。活动播放器独立持有租用，不因别的卡片关闭提前撤销。

没有缩放、压缩或删除原素材。旧记录直接内联的原始 data URL 仍保留在持久数据中，不属于可撤销的 Object URL 缓存。桌面停止自动清理原 Blob 会增加磁盘保留量；后续清理必须统计所有历史引用。
