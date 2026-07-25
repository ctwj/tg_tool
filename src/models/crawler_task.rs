//! CrawlerTask / CrawlerTaskInput 模型（feature 043-crawler-configurator）
//!
//! 043 已删除旧 `selectors` JSON 列（直接取代 042 抓取路径），字段配置由新表
//! `crawler_task_field_nodes` 承载（见 data-model.md E1/E3）。

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::services::crawler::field_schema::FieldTree;

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
    /// 045：URL 模板分页模板（含 {page} 占位符）；空串=未启用（走字段树 pagination 分页）
    pub page_url_template: String,
    /// 045：模板生成页码起始值（默认 1）
    pub page_start: i64,
    /// 045：模板生成页码上限（0=不限）
    pub page_end: i64,
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
    /// 045：URL 模板分页模板（含 {page} 占位符）；空串=未启用（走字段树 pagination 分页）
    #[serde(default)]
    pub page_url_template: String,
    /// 045：模板生成页码起始值（默认 1）
    #[serde(default = "default_one")]
    pub page_start: i64,
    /// 045：模板生成页码上限（0=不限）
    #[serde(default)]
    pub page_end: i64,

    /// 导出/导入携带的字段树（嵌套 spec + children）。
    /// 仅 `import_task` 消费（写入字段节点）；`create_task` / `update_task` /
    /// `from_template` 忽略。导出文件含此字段；旧文件缺省 → None，向后兼容。
    #[serde(default)]
    pub field_tree: Option<FieldTree>,
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
        let template_mode = !self.page_url_template.is_empty();
        // 045：入口模式（无模板）必须填 list_urls；模板模式 list_urls 可空（直接从模板第 1 页抓）
        if !template_mode && self.list_urls.is_empty() {
            return Err(
                "未配置 URL 模板时 list_urls（入口）不能为空；若用 URL 模板分页请填写模板".into(),
            );
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
        // 045：page_start/page_end 基本范围（始终校验，防负数 / 非法起始）
        if self.page_start < 1 {
            return Err("起始页码 page_start 必须 >= 1".into());
        }
        if self.page_end < 0 {
            return Err("终止页码 page_end 必须 >= 0（0 表示不限）".into());
        }
        // 045：URL 模板分页配置校验（模板模式）
        if template_mode {
            let tpl = &self.page_url_template;
            if tpl.matches("{page}").count() != 1 {
                return Err("URL 模板必须含且仅含一个 {page} 占位符".into());
            }
            // 除 {page} 外不得有未配对/非法花括号
            let stripped = tpl.replace("{page}", "");
            if stripped.matches('{').count() != 0 || stripped.matches('}').count() != 0 {
                return Err("URL 模板含非法花括号".into());
            }
            // 模板模式由 page_end 独占翻页边界（忽略 max_pagination_depth），必须 > 0
            if self.page_end <= 0 {
                return Err("URL 模板模式必须设置 page_end（终止页码 > 0）作为翻页边界".into());
            }
            if self.page_end < self.page_start {
                return Err("终止页码 page_end 不能小于起始页码 page_start".into());
            }
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
        let input: CrawlerTaskInput =
            serde_json::from_str(r#"{"name":"t","list_urls":["https://x.com"]}"#)
                .expect("反序列化成功");
        assert!(
            input.force_full_collect,
            "缺省 force_full_collect 必须默认 true"
        );
    }

    #[test]
    fn force_full_collect_respects_explicit_false() {
        let input: CrawlerTaskInput = serde_json::from_str(
            r#"{"name":"t","list_urls":["https://x.com"],"force_full_collect":false}"#,
        )
        .expect("反序列化成功");
        assert!(!input.force_full_collect);
    }

    /// 045：构造一个默认合法的 input，测试时按需覆盖字段
    fn minimal_valid_input() -> CrawlerTaskInput {
        CrawlerTaskInput {
            name: "t".into(),
            enabled: true,
            list_urls: vec!["https://x.com".into()],
            two_stage: true,
            interval_minutes: 30,
            task_concurrency: 1,
            user_agent: None,
            request_delay_ms: 1000,
            proxy: None,
            auto_link_check: false,
            block_detection_config: None,
            max_consecutive_failures: 3,
            template_source: None,
            pagination_selector: None,
            max_pages: 0,
            max_pagination_depth: 10,
            force_full_collect: true,
            page_url_template: String::new(),
            page_start: 1,
            page_end: 0,
            field_tree: None,
        }
    }

    #[test]
    fn validate_rejects_template_without_placeholder() {
        let mut i = minimal_valid_input();
        i.page_url_template = "https://site.com/page-4.html".into();
        assert!(i.validate().is_err(), "无 {{page}} 占位符必须拒绝");
    }

    #[test]
    fn validate_rejects_template_with_multiple_placeholders() {
        let mut i = minimal_valid_input();
        i.page_url_template = "{page}/{page}".into();
        assert!(i.validate().is_err(), "多个 {{page}} 占位符必须拒绝");
    }

    #[test]
    fn validate_rejects_template_with_stray_braces() {
        let mut i = minimal_valid_input();
        i.page_url_template = "page-{page}-{other}".into();
        assert!(i.validate().is_err(), "除 {{page}} 外的花括号必须拒绝");
    }

    #[test]
    fn validate_rejects_page_start_below_one() {
        let mut i = minimal_valid_input();
        i.page_start = 0;
        assert!(i.validate().is_err(), "page_start < 1 必须拒绝");
    }

    #[test]
    fn validate_rejects_page_end_below_start() {
        let mut i = minimal_valid_input();
        i.page_url_template = "page-{page}.html".into();
        i.page_start = 5;
        i.page_end = 3;
        assert!(i.validate().is_err(), "page_end < page_start 必须拒绝");
    }

    #[test]
    fn validate_accepts_valid_template_config() {
        let mut i = minimal_valid_input();
        i.page_url_template = "https://site.com/page-{page}.html".into();
        i.page_start = 2;
        i.page_end = 50;
        assert!(i.validate().is_ok(), "合法模板配置应通过");
    }

    #[test]
    fn validate_accepts_empty_template() {
        let i = minimal_valid_input();
        // 空模板 = 未启用模板分页（入口模式），跳过模板校验
        assert!(i.validate().is_ok(), "空模板（入口模式）应通过");
    }

    /// 045：模板模式允许 list_urls 为空（纯模板，直接从 page_start 抓）
    #[test]
    fn validate_accepts_template_mode_without_list_urls() {
        let mut i = minimal_valid_input();
        i.list_urls = vec![];
        i.page_url_template = "https://site.com/page-{page}.html".into();
        i.page_start = 1;
        i.page_end = 100;
        assert!(i.validate().is_ok(), "纯模板模式允许 list_urls 为空");
    }

    /// 045：模板模式必须 page_end > 0（page_end 独占翻页边界）
    #[test]
    fn validate_rejects_template_mode_with_zero_page_end() {
        let mut i = minimal_valid_input();
        i.page_url_template = "https://site.com/page-{page}.html".into();
        i.page_end = 0;
        assert!(
            i.validate().is_err(),
            "模板模式 page_end=0 应拒绝（需终止边界）"
        );
    }

    /// 045：入口模式（无模板）list_urls 为空应拒绝
    #[test]
    fn validate_rejects_dom_mode_with_empty_list_urls() {
        let mut i = minimal_valid_input();
        i.list_urls = vec![];
        // page_url_template 保持空 = 入口模式
        assert!(
            i.validate().is_err(),
            "入口模式（无模板）list_urls 为空应拒绝"
        );
    }
}
