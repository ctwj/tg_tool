//! 站点模板（内置 + 自定义）— Phase 3 T024 / Phase 6 T052/T053 扩展

use crate::models::crawler_task::CrawlerTaskInput;
use crate::services::crawler::extractor::{FieldSelector, FieldSelectors};
use serde::{Deserialize, Serialize};

/// 内置/自定义模板（与 contracts/crawler-api.md §CrawlerTemplate 对齐）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlerTemplate {
    /// 内置模板标识，如 `generic_resource_site`
    pub key: String,
    pub name: String,
    /// `forum` | `blog` | `resource`
    pub site_type: String,
    pub description: String,
    /// 完整配置（用户仅需改 list_urls）
    pub config: CrawlerTaskInput,
}

/// 默认 User-Agent — 真实浏览器 UA 字符串（research.md §options table）
pub const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/130.0.0.0 Safari/537.36";

/// 内置模板列表（Phase 3：1 个通用资源站；Phase 6 补齐 discuz_forum / wordpress_blog）
pub fn builtin_templates() -> Vec<CrawlerTemplate> {
    vec![
        CrawlerTemplate {
            key: "generic_resource_site".into(),
            name: "通用资源站".into(),
            site_type: "resource".into(),
            description: "通用资源下载站模板：两阶段抓取，标题/正文/网盘/直链/图片全字段。\
                          用户需根据目标站实际 HTML 调整 CSS 选择器。"
                .into(),
            config: generic_resource_site_config(),
        },
        CrawlerTemplate {
            key: "discuz_forum".into(),
            name: "Discuz! 论坛".into(),
            site_type: "forum".into(),
            description: "Discuz! X3.x 论坛帖子抓取：两阶段，列表页 threadlist 主题列表 → 帖子详情。\
                          自动识别常见网盘域名（quark/baidu/aliyun 等）。适用于大多数 Discuz 资源站。"
                .into(),
            config: discuz_forum_config(),
        },
        CrawlerTemplate {
            key: "wordpress_blog".into(),
            name: "WordPress 博客".into(),
            site_type: "blog".into(),
            description: "WordPress 经典主题/区块主题博客文章抓取：两阶段，列表页 article → 单篇文章。\
                          适配 .post / .entry-content / wp-block-* 等常见类名。"
                .into(),
            config: wordpress_blog_config(),
        },
    ]
}

fn generic_resource_site_config() -> CrawlerTaskInput {
    CrawlerTaskInput {
        name: "new-resource-site".into(),
        enabled: true,
        list_urls: vec!["https://example-resources.com/list".into()],
        selectors: FieldSelectors {
            list_item: ".post-list .post-item".into(),
            detail_link: "a.detail-link".into(),
            detail_link_attr: Some("href".into()),
            title: FieldSelector {
                css: "h1.post-title".into(),
                attr: None,
                regex: None,
            },
            content: FieldSelector {
                css: ".post-content".into(),
                attr: Some("html".into()),
                regex: None,
            },
            category: FieldSelector {
                css: ".post-category".into(),
                ..Default::default()
            },
            tags: FieldSelector {
                css: ".post-tags".into(),
                ..Default::default()
            },
            images: FieldSelector {
                css: ".post-content img".into(),
                attr: Some("src".into()),
                ..Default::default()
            },
            pan_links: FieldSelector {
                css: ".download-links a".into(),
                attr: Some("href".into()),
                ..Default::default()
            },
            direct_links: FieldSelector {
                css: ".direct-download a".into(),
                attr: Some("href".into()),
                ..Default::default()
            },
        },
        two_stage: true,
        interval_minutes: 30,
        task_concurrency: 1,
        user_agent: Some(DEFAULT_USER_AGENT.into()),
        request_delay_ms: 1000,
        proxy: None,
        auto_link_check: false,
        block_detection_config: None,
        max_consecutive_failures: 3,
        template_source: Some("generic_resource_site".into()),
        // 通用资源站常见分页容器：抓所有数字 + 下一页
        pagination_selector: Some(".pagination a, a.next, a[rel=next]".into()),
        max_pages: 0,
    }
}

/// Discuz! X3.x 论坛模板（T052）— list_item 用 `.threadlist li`、详情链接 `.ic2 a.xst`
fn discuz_forum_config() -> CrawlerTaskInput {
    CrawlerTaskInput {
        name: "discuz-forum".into(),
        enabled: true,
        list_urls: vec!["https://forum.example.com/forum-12-1".into()],
        selectors: FieldSelectors {
            // Discuz threadlist 列表项
            list_item: "#threadlist .threadlist tbody[id^='normalthread']".into(),
            detail_link: "tr th a.s.xst".into(),
            detail_link_attr: Some("href".into()),
            title: FieldSelector {
                css: "#thread_subject".into(),
                attr: None,
                regex: None,
            },
            content: FieldSelector {
                css: ".t_fszd .t_fsz, td.t_f".into(),
                attr: Some("html".into()),
                regex: None,
            },
            category: FieldSelector {
                css: "#pt .z a:nth-last-child(2)".into(),
                ..Default::default()
            },
            tags: FieldSelector {
                css: "".into(),
                ..Default::default()
            },
            images: FieldSelector {
                css: ".t_fszd .t_f img, td.t_f img".into(),
                attr: Some("file".into()), // Discuz 懒加载 attr 优先；fallback 由 engine 处理 src
                ..Default::default()
            },
            // 论坛网盘链接通常在帖子正文或隐藏的 reply 区域
            pan_links: FieldSelector {
                css: ".t_fszd .t_f a, td.t_f a, .locked a".into(),
                attr: Some("href".into()),
                ..Default::default()
            },
            direct_links: FieldSelector {
                css: "attachimgright a, .attnm a".into(),
                attr: Some("href".into()),
                ..Default::default()
            },
        },
        two_stage: true,
        interval_minutes: 60,
        task_concurrency: 1,
        user_agent: Some(DEFAULT_USER_AGENT.into()),
        request_delay_ms: 1500, // 论坛更敏感，间隔放大
        proxy: None,
        auto_link_check: false,
        block_detection_config: None,
        max_consecutive_failures: 3,
        template_source: Some("discuz_forum".into()),
        // Discuz! X3.x 分页容器 .pg：抓所有页码 + next
        pagination_selector: Some(".pg a".into()),
        max_pages: 0,
    }
}

/// WordPress 经典/区块主题模板（T052）
fn wordpress_blog_config() -> CrawlerTaskInput {
    CrawlerTaskInput {
        name: "wordpress-blog".into(),
        enabled: true,
        list_urls: vec!["https://blog.example.com".into()],
        selectors: FieldSelectors {
            // 兼容经典主题 article.post 与区块主题 .wp-block-post
            list_item: "article.post, .wp-block-post".into(),
            detail_link: "h2.entry-title a, h2 a[href*='/'], .entry-title a".into(),
            detail_link_attr: Some("href".into()),
            title: FieldSelector {
                css: "h1.entry-title".into(),
                attr: None,
                regex: None,
            },
            content: FieldSelector {
                css: ".entry-content, .wp-block-post-content".into(),
                attr: Some("html".into()),
                regex: None,
            },
            category: FieldSelector {
                css: ".cat-links a, .post-categories a".into(),
                ..Default::default()
            },
            tags: FieldSelector {
                css: ".tags-links a, .tagcloud a".into(),
                ..Default::default()
            },
            images: FieldSelector {
                css: ".entry-content img, .wp-block-post-content img".into(),
                attr: Some("src".into()),
                ..Default::default()
            },
            pan_links: FieldSelector {
                css: ".entry-content a, .wp-block-post-content a".into(),
                attr: Some("href".into()),
                ..Default::default()
            },
            direct_links: FieldSelector {
                css: ".entry-content a[href$='.zip'], .entry-content a[href$='.rar'], .entry-content a[href$='.pdf']".into(),
                attr: Some("href".into()),
                ..Default::default()
            },
        },
        two_stage: true,
        interval_minutes: 30,
        task_concurrency: 1,
        user_agent: Some(DEFAULT_USER_AGENT.into()),
        request_delay_ms: 1000,
        proxy: None,
        auto_link_check: false,
        block_detection_config: None,
        max_consecutive_failures: 3,
        template_source: Some("wordpress_blog".into()),
        // WordPress 经典/区块主题分页：抓所有数字 + next/prev
        pagination_selector: Some(".nav-links a, .pagination a, a[rel=next]".into()),
        max_pages: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_templates_nonempty() {
        let t = builtin_templates();
        assert!(!t.is_empty());
        assert!(t.iter().any(|x| x.key == "generic_resource_site"));
    }

    #[test]
    fn builtin_templates_count_meets_us4() {
        // T052：补齐至 3 个内置模板
        let t = builtin_templates();
        assert!(t.len() >= 3, "expected >= 3 builtin templates, got {}", t.len());
        let keys: Vec<&str> = t.iter().map(|x| x.key.as_str()).collect();
        assert!(keys.contains(&"discuz_forum"), "missing discuz_forum");
        assert!(keys.contains(&"wordpress_blog"), "missing wordpress_blog");
        assert!(keys.contains(&"generic_resource_site"), "missing generic_resource_site");
    }

    #[test]
    fn discuz_config_valid_and_tagged() {
        let t = builtin_templates().into_iter().find(|x| x.key == "discuz_forum").unwrap();
        assert_eq!(t.site_type, "forum");
        t.config.validate().expect("discuz config should be valid");
        assert_eq!(t.config.template_source.as_deref(), Some("discuz_forum"));
        assert!(t.config.request_delay_ms >= 1000, "forum should be conservative");
    }

    #[test]
    fn wordpress_config_valid_and_tagged() {
        let t = builtin_templates().into_iter().find(|x| x.key == "wordpress_blog").unwrap();
        assert_eq!(t.site_type, "blog");
        t.config.validate().expect("wordpress config should be valid");
        assert_eq!(t.config.template_source.as_deref(), Some("wordpress_blog"));
        // 区块主题 fallback
        assert!(t.config.selectors.list_item.contains("wp-block-post"));
    }

    #[test]
    fn generic_config_valid() {
        let c = generic_resource_site_config();
        c.validate().expect("generic config should be valid");
        // template_source 是模板 key，site_type 在模板元数据层而非 config
        assert_eq!(c.template_source.as_deref(), Some("generic_resource_site"));
    }

    #[test]
    fn template_serializes_to_contract() {
        // 与 contracts/crawler-api.md §CrawlerTemplate 结构一致
        let t = &builtin_templates()[0];
        let json = serde_json::to_value(t).unwrap();
        assert!(json.get("key").is_some());
        assert!(json.get("name").is_some());
        assert!(json.get("site_type").is_some());
        assert!(json.get("description").is_some());
        assert!(json.get("config").is_some());
    }

    // ===== 自动翻页：内置模板预填 pagination_selector =====

    #[test]
    fn all_builtin_templates_have_pagination_selector() {
        // 用户从内置模板创建任务即可享受自动翻页，无需手动配置
        for t in builtin_templates() {
            assert!(
                t.config.pagination_selector.as_deref().is_some_and(|s| !s.is_empty()),
                "template {} should pre-fill pagination_selector",
                t.key
            );
            assert_eq!(t.config.max_pages, 0, "template {} max_pages should be 0 (unlimited)", t.key);
        }
    }

    #[test]
    fn discuz_template_pagination_selector_targets_pg() {
        let t = builtin_templates().into_iter().find(|x| x.key == "discuz_forum").unwrap();
        let sel = t.config.pagination_selector.unwrap();
        assert!(sel.contains(".pg"), "discuz selector should target .pg container, got: {sel}");
    }

    #[test]
    fn wordpress_template_pagination_selector_includes_rel_next() {
        let t = builtin_templates().into_iter().find(|x| x.key == "wordpress_blog").unwrap();
        let sel = t.config.pagination_selector.unwrap();
        // 应该至少有一个常见 WordPress 分页 class 或 rel=next
        assert!(
            sel.contains("nav-links") || sel.contains("pagination") || sel.contains("rel=next"),
            "wordpress selector should target common pagination classes, got: {sel}"
        );
    }
}
