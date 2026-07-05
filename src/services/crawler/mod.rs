//! Crawler subsystem — feature 042-web-crawler-collector
//!
//! 完全独立于现有 Telegram 采集系统，基于配置驱动的多站点网络爬虫。
//! 子模块：
//! - [`url_normalize`]: URL 规范化（去 utm、参数排序等，research.md R2）
//! - [`pan_detector`]: 9 平台网盘识别 + 提取码关联（research.md R6）
//! - [`block_detector`]: 反爬拦截识别（5 类信号，research.md R5）
//! - [`extractor`]: HTML 字段提取（CSS 选择器 + 正则后处理，research.md R1）
//! - [`scheduler`]: 任务调度器（30s tick + Semaphore 并发控制，research.md R4）
//! - [`engine`]: 单任务抓取引擎（列表页 → 详情页，落库）
//! - [`image_uploader`]: 图片下载→上传图床群组异步管线（research.md R3+R7）
//! - [`templates`]: 内置 + 自定义站点模板
//! - [`script_engine`] / [`script_runner`] / [`script_fetch`]: feature 046-crawler-script-extractor
//!   第 7 种提取模式 script（rquickjs 沙箱），脚本字段 + ctx.fetch（含 SSRF 防护）
//! - [`refresh`]: feature 046 按需懒刷新（lazy refresh on read）

pub mod block_detector;
pub mod engine;
pub mod extractor;
pub mod field_schema;
pub mod follow_url;
pub mod image_uploader;
pub mod pan_detector;
pub mod preset_library;
pub mod probe;
pub mod refresh;
pub mod scheduler;
pub mod script_engine;
pub mod script_fetch;
pub mod script_runner;
pub mod source_layer;
pub mod templates;
pub mod url_normalize;

// 便捷重导出：handler/state 直接 `use crate::services::crawler::CrawlerSchedulerHandle`
pub use scheduler::{create_scheduler, CrawlerSchedulerHandle, CrawlerSchedulerState};
