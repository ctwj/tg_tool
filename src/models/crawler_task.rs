//! CrawlerTask / CrawlerTaskInput 模型（feature 043-crawler-configurator）
//!
//! 043 已删除旧 `selectors` JSON 列（直接取代 042 抓取路径），字段配置由新表
//! `crawler_task_field_nodes` 承载（见 data-model.md E1/E3）。

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// 爬虫任务 — 每条记录代表一个独立的网站爬虫配置
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CrawlerTask {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    /// JSON 数组（字符串形式）— 列表页 URL 列表
    pub list_urls: String,
    /// 历史字段：单阶段模式已下线，DB 列保留兼容，值恒为 true
    pub two_stage: bool,
    pub interval_minutes: i64,
    pub task_concurrency: i64,
    pub user_agent: Option<String>,
    pub request_delay_ms: i64,
    pub proxy: Option<String>,
    pub auto_link_check: bool,
    /// JSON — 自定义拦截关键词覆盖
    pub block_detection_config: Option<String>,
    pub max_consecutive_failures: i64,
    pub template_source: Option<String>,
    /// CSS 选择器：一次性匹配页面所有分页链接（如 .pagination a / a[rel=next]）。
    /// 引擎把所有命中的 href 去重后批量抓取。NULL=未启用
    pub pagination_selector: Option<String>,
    /// 最大抓取页数（含 list_urls 中的种子页），0 表示不限
    pub max_pages: i64,
    /// 043 US5：字段树 pagination 字段驱动的最大翻页深度，默认 10（FR-022）；0=不限
    pub max_pagination_depth: i64,
    /// 044：全量采集开关。true=每次全量（跑满 max_pagination_depth/翻完，失败重跑也全量）；
    /// false=连续 3 页零新增时自动早停（适合已成功全量一次后的增量维护）
    pub force_full_collect: bool,
    pub status: String,
    pub consecutive_failures: i64,
    pub last_run_at: Option<NaiveDateTime>,
    pub next_run_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// 创建/更新任务时的输入 body（不含 id/时间戳/status 计数字段）
///
/// 用于 `POST /api/crawler/tasks`、`PUT /api/crawler/tasks/{id}`、`POST /api/crawler/tasks/import`
///
/// **043 变更**：移除 `selectors` 字段（DB 列已删），字段配置通过独立的
/// `/api/crawler/tasks/{id}/field-nodes` CRUD 管理。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlerTaskInput {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub list_urls: Vec<String>,
    /// 历史字段：单阶段模式已下线，DB 列保留兼容；序列化/反序列化兼容老数据，新代码强制 true
    #[serde(default = "default_true")]
    pub two_stage: bool,
    #[serde(default = "default_interval")]
    pub interval_minutes: i64,
    #[serde(default = "default_one")]
    pub task_concurrency: i64,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default = "default_delay")]
    pub request_delay_ms: i64,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub auto_link_check: bool,
    #[serde(default)]
    pub block_detection_config: Option<String>,
    #[serde(default = "default_three")]
    pub max_consecutive_failures: i64,
    #[serde(default)]
    pub template_source: Option<String>,
    /// CSS 选择器：一次性匹配页面所有分页链接（如 .pagination a / a[rel=next]）。
    /// 空/None = 未启用自动翻页
    #[serde(default)]
    pub pagination_selector: Option<String>,
    /// 最大抓取页数（0=不限）
    #[serde(default)]
    pub max_pages: i64,
    /// 043 US5：字段树 pagination 字段驱动的最大翻页深度，默认 10（FR-022）；0=不限
    #[serde(default = "default_ten")]
    pub max_pagination_depth: i64,
    /// 044：全量采集开关（默认 true）。true=每次全量；false=连续 3 页零新增早停
    #[serde(default = "default_true")]
    pub force_full_collect: bool,
}

fn default_true() -> bool {
    true
}
fn default_one() -> i64 {
    1
}
fn default_three() -> i64 {
    3
}
fn default_interval() -> i64 {
    30
}
fn default_delay() -> i64 {
    1000
}
fn default_ten() -> i64 {
    10
}

impl CrawlerTaskInput {
    /// 将 list_urls 序列化为 JSON 字符串（DB 存储）
    pub fn list_urls_json(&self) -> String {
        serde_json::to_string(&self.list_urls).unwrap_or_else(|_| "[]".to_string())
    }

    /// 校验输入合法性（返回 first-error）
    ///
    /// **043 变更**：移除 selectors 校验（字段树由 `/field-nodes` 独立 CRUD 校验）。
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("任务名不能为空".into());
        }
        if self.list_urls.is_empty() {
            return Err("list_urls 不能为空".into());
        }
        if self.interval_minutes < 1 {
            return Err("interval_minutes 必须 >= 1".into());
        }
        if self.task_concurrency < 1 {
            return Err("task_concurrency 必须 >= 1".into());
        }
        if self.max_consecutive_failures < 1 {
            return Err("max_consecutive_failures 必须 >= 1".into());
        }
        if self.max_pages < 0 {
            return Err("max_pages 必须 >= 0（0 表示不限）".into());
        }
        if self.max_pagination_depth < 0 {
            return Err("max_pagination_depth 必须 >= 0（0 表示不限）".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::CrawlerTaskInput;

    #[test]
    fn force_full_collect_defaults_to_true_when_absent() {
        // body 不传 force_full_collect 时，serde 默认应为 true（开关 ON = 全量采集）
        let input: CrawlerTaskInput = serde_json::from_str(
            r#"{"name":"t","list_urls":["https://x.com"]}"#,
        )
        .expect("反序列化成功");
        assert!(input.force_full_collect, "缺省 force_full_collect 必须默认 true");
    }

    #[test]
    fn force_full_collect_respects_explicit_false() {
        let input: CrawlerTaskInput = serde_json::from_str(
            r#"{"name":"t","list_urls":["https://x.com"],"force_full_collect":false}"#,
        )
        .expect("反序列化成功");
        assert!(!input.force_full_collect);
    }
}
