//! 内置站点模板（feature 043-crawler-configurator）
//!
//! 直接取代 042 旧的 `CrawlerTemplate` / `generic_resource_site_config` /
//! `discuz_forum_config` / `wordpress_blog_config`。
//!
//! 新模板基于 **FieldTree**（字段树）— 每个模板含一棵预置好的字段树，
//! 用户从模板创建任务时直接展开为 `crawler_task_field_nodes` 行。
//!
//! 模板列表：
//! - `discuz_forum` — Discuz! 论坛（帖子列表 + 帖子详情）
//! - `wordpress_blog` — WordPress 博客（文章列表 + 文章详情）
//! - `generic_resource_site` — 通用资源站（列表 + 详情，含 cover/description/tags）

use serde::{Deserialize, Serialize};

use crate::services::crawler::field_schema::{
    ExtractorMode, FieldNodeSpec, FieldTree, FieldTreeNode, FieldType, PostProcessor,
    PostProcessorOp, Rule, Scope, SourceLayer, validate_name, validate_rule,
};

// ============================================================================
// 数据结构
// ============================================================================

/// 内置模板对外暴露的结构（GET /api/crawler/task-templates 的元素）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinTemplate {
    /// 模板唯一 key（用作 from-template 入参）
    pub key: &'static str,
    /// 展示名称
    pub name: &'static str,
    /// 适用站点说明
    pub description: &'static str,
    /// 站点类型（与 042 source_type 兼容，便于推送接入识别）
    pub source_type: &'static str,
    /// 预置字段树
    pub field_tree: FieldTree,
}

// ============================================================================
// PostProcessor / Spec 构造快捷宏（仅本文件内部使用）
// ============================================================================

/// 构造 PostProcessor
const fn pp(op: PostProcessorOp) -> PostProcessor {
    PostProcessor { op }
}

/// 构造一个字段节点 spec（顶层；不含 id/task_id/parent_id，由插入时回填）
#[allow(clippy::too_many_arguments)]
fn spec(
    scope: Scope,
    name: &str,
    display_name: &str,
    field_type: FieldType,
    source_layer: SourceLayer,
    extractor_mode: ExtractorMode,
    rule: Rule,
    post_processors: Vec<PostProcessor>,
    sort_order: i32,
) -> FieldNodeSpec {
    FieldNodeSpec {
        id: None,
        task_id: None,
        parent_id: None,
        scope,
        name: name.into(),
        display_name: display_name.into(),
        field_type,
        source_layer,
        extractor_mode,
        rule,
        post_processors,
        script_index: None,
        sort_order,
        is_active: true,
        refresh_on_read: false,
    }
}

// ============================================================================
// 模板 1：Discuz! 论坛
// ============================================================================
//
// 列表页：论坛板块页 `.forumlist tbody tr` 或帖子列表 `tbody[id^=normalthread_]` 行
// 详情页：帖子页 `<td.t_f>` 主体内容
fn discuz_forum() -> FieldTree {
    use crate::services::crawler::field_schema::{
        CssRule, ExtractorMode::Css, FieldType::*, Scope::*, SourceLayer::Html,
    };

    FieldTree {
        list_page: vec![
            // 链接卡片：每个帖子一行，含标题链接 + 缩略图（若有）
            FieldTreeNode {
                spec: spec(
                    ListPage,
                    "thread_card",
                    "帖子卡片",
                    LinkCard,
                    Html,
                    Css,
                    Rule::Css(CssRule {
                        selector: "tbody[id^=normalthread_] a.s.xst, .forumlist a.xst".into(),
                        attr: "href".into(),
                    }),
                    vec![],
                    0,
                ),
                children: vec![
                    FieldTreeNode {
                        spec: spec(
                            ListPage,
                            "title",
                            "帖子标题",
                            String,
                            Html,
                            Css,
                            Rule::Css(CssRule {
                                selector: "a.xst".into(),
                                attr: "text".into(),
                            }),
                            vec![pp(PostProcessorOp::Trim)],
                            0,
                        ),
                        children: vec![],
                    },
                    FieldTreeNode {
                        spec: spec(
                            ListPage,
                            "cover",
                            "帖子封面",
                            Image,
                            Html,
                            Css,
                            Rule::Css(CssRule {
                                selector: "img.attach".into(),
                                attr: "src".into(),
                            }),
                            vec![],
                            1,
                        ),
                        children: vec![],
                    },
                ],
            },
        ],
        detail_page: vec![
            FieldTreeNode {
                spec: spec(
                    DetailPage,
                    "title",
                    "帖子标题",
                    String,
                    Html,
                    Css,
                    Rule::Css(CssRule {
                        selector: "#thread_subject, h1.title".into(),
                        attr: "text".into(),
                    }),
                    vec![pp(PostProcessorOp::Trim)],
                    0,
                ),
                children: vec![],
            },
            FieldTreeNode {
                spec: spec(
                    DetailPage,
                    "content",
                    "帖子正文",
                    Text,
                    Html,
                    Css,
                    Rule::Css(CssRule {
                        selector: "td.t_f, .message".into(),
                        attr: "text".into(),
                    }),
                    vec![pp(PostProcessorOp::Trim)],
                    1,
                ),
                children: vec![],
            },
            FieldTreeNode {
                spec: spec(
                    DetailPage,
                    "post_time",
                    "发帖时间",
                    Datetime,
                    Html,
                    Css,
                    Rule::Css(CssRule {
                        selector: ".authi em, #authorposton1".into(),
                        attr: "text".into(),
                    }),
                    vec![pp(PostProcessorOp::Trim)],
                    2,
                ),
                children: vec![],
            },
        ],
    }
}

// ============================================================================
// 模板 2：WordPress 博客
// ============================================================================
//
// 列表页：`.post` 文章卡片
// 详情页：`.entry-title` + `.entry-content`
fn wordpress_blog() -> FieldTree {
    use crate::services::crawler::field_schema::{
        CssRule, ExtractorMode::Css, FieldType::*, Scope::*, SourceLayer::Html,
    };

    FieldTree {
        list_page: vec![FieldTreeNode {
            spec: spec(
                ListPage,
                "post_card",
                "文章卡片",
                LinkCard,
                Html,
                Css,
                Rule::Css(CssRule {
                    selector: ".post h2 a, .entry-title a".into(),
                    attr: "href".into(),
                }),
                vec![],
                0,
            ),
            children: vec![
                FieldTreeNode {
                    spec: spec(
                        ListPage,
                        "title",
                        "文章标题",
                        String,
                        Html,
                        Css,
                        Rule::Css(CssRule {
                            selector: "a".into(),
                            attr: "text".into(),
                        }),
                        vec![pp(PostProcessorOp::Trim)],
                        0,
                    ),
                    children: vec![],
                },
                FieldTreeNode {
                    spec: spec(
                        ListPage,
                        "cover",
                        "封面图",
                        Image,
                        Html,
                        Css,
                        Rule::Css(CssRule {
                            selector: "img.wp-post-image, img.attachment-post-thumbnail".into(),
                            attr: "src".into(),
                        }),
                        vec![],
                        1,
                    ),
                    children: vec![],
                },
            ],
        }],
        detail_page: vec![
            FieldTreeNode {
                spec: spec(
                    DetailPage,
                    "title",
                    "文章标题",
                    String,
                    Html,
                    Css,
                    Rule::Css(CssRule {
                        selector: "h1.entry-title".into(),
                        attr: "text".into(),
                    }),
                    vec![pp(PostProcessorOp::Trim)],
                    0,
                ),
                children: vec![],
            },
            FieldTreeNode {
                spec: spec(
                    DetailPage,
                    "content",
                    "正文",
                    Text,
                    Html,
                    Css,
                    Rule::Css(CssRule {
                        selector: ".entry-content".into(),
                        attr: "text".into(),
                    }),
                    vec![pp(PostProcessorOp::Trim)],
                    1,
                ),
                children: vec![],
            },
            FieldTreeNode {
                spec: spec(
                    DetailPage,
                    "published_at",
                    "发布时间",
                    Datetime,
                    Html,
                    Css,
                    Rule::Css(CssRule {
                        selector: "time.entry-date, .post-date".into(),
                        attr: "datetime".into(),
                    }),
                    vec![pp(PostProcessorOp::Trim)],
                    2,
                ),
                children: vec![],
            },
            FieldTreeNode {
                spec: spec(
                    DetailPage,
                    "tags",
                    "标签",
                    String,
                    Html,
                    Css,
                    Rule::Css(CssRule {
                        selector: ".tagcloud a, .post-tags a".into(),
                        attr: "text".into(),
                    }),
                    vec![pp(PostProcessorOp::Trim), pp(PostProcessorOp::Dedupe)],
                    3,
                ),
                children: vec![],
            },
        ],
    }
}

// ============================================================================
// 模板 3：通用资源站
// ============================================================================
//
// 适配大多数「列表 + 详情」结构的资源站，列表卡片含 cover/title/description/tags。
fn generic_resource_site() -> FieldTree {
    use crate::services::crawler::field_schema::{
        CssRule, ExtractorMode::Css, FieldType::*, Scope::*, SourceLayer::Html,
    };

    FieldTree {
        list_page: vec![FieldTreeNode {
            spec: spec(
                ListPage,
                "resource_card",
                "资源卡片",
                LinkCard,
                Html,
                Css,
                Rule::Css(CssRule {
                    selector: ".item, .card, .post, article".into(),
                    attr: "html".into(),
                }),
                vec![],
                0,
            ),
            children: vec![
                FieldTreeNode {
                    spec: spec(
                        ListPage,
                        "title",
                        "标题",
                        String,
                        Html,
                        Css,
                        Rule::Css(CssRule {
                            selector: "a.title, h2 a, h3 a, .title a".into(),
                            attr: "text".into(),
                        }),
                        vec![pp(PostProcessorOp::Trim)],
                        0,
                    ),
                    children: vec![],
                },
                FieldTreeNode {
                    spec: spec(
                        ListPage,
                        "url",
                        "详情链接",
                        Url,
                        Html,
                        Css,
                        Rule::Css(CssRule {
                            selector: "a.title, h2 a, h3 a, .title a".into(),
                            attr: "href".into(),
                        }),
                        vec![],
                        1,
                    ),
                    children: vec![],
                },
                FieldTreeNode {
                    spec: spec(
                        ListPage,
                        "cover",
                        "封面图",
                        Image,
                        Html,
                        Css,
                        Rule::Css(CssRule {
                            selector: "img, .thumb img, .cover img".into(),
                            attr: "src".into(),
                        }),
                        vec![pp(PostProcessorOp::First)],
                        2,
                    ),
                    children: vec![],
                },
                FieldTreeNode {
                    spec: spec(
                        ListPage,
                        "description",
                        "摘要",
                        Text,
                        Html,
                        Css,
                        Rule::Css(CssRule {
                            selector: ".desc, .summary, .excerpt, p".into(),
                            attr: "text".into(),
                        }),
                        vec![pp(PostProcessorOp::Trim), pp(PostProcessorOp::First)],
                        3,
                    ),
                    children: vec![],
                },
            ],
        }],
        detail_page: vec![
            FieldTreeNode {
                spec: spec(
                    DetailPage,
                    "title",
                    "标题",
                    String,
                    Html,
                    Css,
                    Rule::Css(CssRule {
                        selector: "h1, .title, .article-title".into(),
                        attr: "text".into(),
                    }),
                    vec![pp(PostProcessorOp::Trim)],
                    0,
                ),
                children: vec![],
            },
            FieldTreeNode {
                spec: spec(
                    DetailPage,
                    "content",
                    "正文",
                    Text,
                    Html,
                    Css,
                    Rule::Css(CssRule {
                        selector: ".content, .article-content, .entry-content, article".into(),
                        attr: "text".into(),
                    }),
                    vec![pp(PostProcessorOp::Trim)],
                    1,
                ),
                children: vec![],
            },
            FieldTreeNode {
                spec: spec(
                    DetailPage,
                    "cover",
                    "封面图",
                    Image,
                    Html,
                    Css,
                    Rule::Css(CssRule {
                        selector: ".cover img, .thumbnail img, article img".into(),
                        attr: "src".into(),
                    }),
                    vec![pp(PostProcessorOp::First)],
                    2,
                ),
                children: vec![],
            },
            FieldTreeNode {
                spec: spec(
                    DetailPage,
                    "tags",
                    "标签",
                    String,
                    Html,
                    Css,
                    Rule::Css(CssRule {
                        selector: ".tags a, .tag-list a, .post-tags a".into(),
                        attr: "text".into(),
                    }),
                    vec![pp(PostProcessorOp::Trim), pp(PostProcessorOp::Dedupe)],
                    3,
                ),
                children: vec![],
            },
        ],
    }
}

// ============================================================================
// 模板表
// ============================================================================

/// 内置模板列表（编译期常量；运行期惰性求值）
pub fn builtin_templates() -> &'static [BuiltinTemplate] {
    static TEMPLATES: std::sync::OnceLock<Vec<BuiltinTemplate>> = std::sync::OnceLock::new();
    TEMPLATES.get_or_init(|| {
        vec![
            BuiltinTemplate {
                key: "discuz_forum",
                name: "Discuz! 论坛",
                description: "适用于 Discuz! 论坛：板块帖子列表 + 帖子详情（标题/正文/发帖时间）",
                source_type: "discuz",
                field_tree: discuz_forum(),
            },
            BuiltinTemplate {
                key: "wordpress_blog",
                name: "WordPress 博客",
                description: "适用于 WordPress：文章列表（标题/封面）+ 详情（标题/正文/发布时间/标签）",
                source_type: "wordpress",
                field_tree: wordpress_blog(),
            },
            BuiltinTemplate {
                key: "generic_resource_site",
                name: "通用资源站",
                description: "适配大多数「列表 + 详情」结构：资源卡片（标题/链接/封面/摘要）+ 详情（标题/正文/封面/标签）",
                source_type: "generic",
                field_tree: generic_resource_site(),
            },
        ]
    })
}

/// 按 key 查找模板
pub fn find_template(key: &str) -> Option<&'static BuiltinTemplate> {
    builtin_templates().iter().find(|t| t.key == key)
}

// ============================================================================
// 校验（深度遍历字段树，校验 name + rule）
// ============================================================================

/// 校验单个字段节点及其子树（递归）
fn validate_node(node: &FieldTreeNode) -> Result<(), String> {
    let s = &node.spec;
    validate_name(&s.name)?;
    // Rule 序列化为 {"mode":"css","spec":{...}}，需要取出 "spec" 内层
    let full =
        serde_json::to_value(&s.rule).map_err(|e| format!("rule 序列化失败 ({}): {e}", s.name))?;
    let inner = full
        .get("spec")
        .ok_or_else(|| format!("rule 缺少 spec 字段 ({})", s.name))?;
    let rule_json = serde_json::to_string(inner)
        .map_err(|e| format!("rule inner 序列化失败 ({}): {e}", s.name))?;
    validate_rule(s.extractor_mode, &rule_json)?;
    for child in &node.children {
        validate_node(child)?;
    }
    Ok(())
}

/// 校验字段树（list_page + detail_page 全部节点）
pub fn validate_field_tree(tree: &FieldTree) -> Result<(), String> {
    for node in &tree.list_page {
        validate_node(node)?;
    }
    for node in &tree.detail_page {
        validate_node(node)?;
    }
    Ok(())
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_templates_has_three_entries() {
        let t = builtin_templates();
        assert_eq!(t.len(), 3, "应有 3 个内置模板");
    }

    #[test]
    fn builtin_templates_keys_are_unique() {
        let keys: Vec<&str> = builtin_templates().iter().map(|t| t.key).collect();
        let unique: std::collections::HashSet<&str> = keys.iter().copied().collect();
        assert_eq!(keys.len(), unique.len(), "模板 key 必须唯一");
    }

    #[test]
    fn find_template_returns_correct_entry() {
        assert_eq!(find_template("discuz_forum").unwrap().source_type, "discuz");
        assert_eq!(
            find_template("wordpress_blog").unwrap().source_type,
            "wordpress"
        );
        assert_eq!(
            find_template("generic_resource_site").unwrap().source_type,
            "generic"
        );
        assert!(find_template("non_existent").is_none());
    }

    /// 所有模板的字段树必须通过 field_schema::validate
    #[test]
    fn all_builtin_templates_pass_field_schema_validate() {
        for t in builtin_templates() {
            validate_field_tree(&t.field_tree)
                .unwrap_or_else(|e| panic!("模板 {} 校验失败: {e}", t.key));
        }
    }

    /// 每个模板必须至少有 1 个 list_page 字段 + 1 个 detail_page 字段
    #[test]
    fn all_templates_have_list_and_detail_fields() {
        for t in builtin_templates() {
            assert!(
                !t.field_tree.list_page.is_empty(),
                "模板 {} 缺少 list_page 字段",
                t.key
            );
            assert!(
                !t.field_tree.detail_page.is_empty(),
                "模板 {} 缺少 detail_page 字段",
                t.key
            );
        }
    }

    /// 每个模板 list_page 至少含一个 link_card 类型字段（用于详情链接收集）
    #[test]
    fn all_templates_have_link_card_in_list_page() {
        use crate::services::crawler::field_schema::FieldType;
        for t in builtin_templates() {
            let has_link_card = t.field_tree.list_page.iter().any(|n| {
                n.spec.field_type == FieldType::LinkCard || n.spec.field_type == FieldType::Url
            });
            assert!(
                has_link_card,
                "模板 {} 的 list_page 必须含 link_card 或 url 字段",
                t.key
            );
        }
    }

    /// 字段名必须唯一（同一 scope 内）
    #[test]
    fn field_names_unique_per_scope() {
        fn collect_names(nodes: &[FieldTreeNode], sink: &mut Vec<String>) {
            for n in nodes {
                sink.push(n.spec.name.clone());
                collect_names(&n.children, sink);
            }
        }
        for t in builtin_templates() {
            let mut names = Vec::new();
            collect_names(&t.field_tree.list_page, &mut names);
            let unique: std::collections::HashSet<_> = names.iter().collect();
            assert_eq!(
                names.len(),
                unique.len(),
                "模板 {} 的 list_page 字段名重复",
                t.key
            );

            let mut names = Vec::new();
            collect_names(&t.field_tree.detail_page, &mut names);
            let unique: std::collections::HashSet<_> = names.iter().collect();
            assert_eq!(
                names.len(),
                unique.len(),
                "模板 {} 的 detail_page 字段名重复",
                t.key
            );
        }
    }

    #[test]
    fn discuz_forum_template_complete() {
        let t = find_template("discuz_forum").expect("discuz_forum 模板存在");
        assert!(
            t.field_tree
                .list_page
                .iter()
                .any(|n| n.spec.name == "thread_card")
        );
        assert!(
            t.field_tree
                .detail_page
                .iter()
                .any(|n| n.spec.name == "title")
        );
        assert!(
            t.field_tree
                .detail_page
                .iter()
                .any(|n| n.spec.name == "content")
        );
    }

    #[test]
    fn wordpress_blog_template_complete() {
        let t = find_template("wordpress_blog").expect("wordpress_blog 模板存在");
        assert!(
            t.field_tree
                .list_page
                .iter()
                .any(|n| n.spec.name == "post_card")
        );
        assert!(
            t.field_tree
                .detail_page
                .iter()
                .any(|n| n.spec.name == "title")
        );
        assert!(
            t.field_tree
                .detail_page
                .iter()
                .any(|n| n.spec.name == "content")
        );
    }

    #[test]
    fn generic_resource_site_template_complete() {
        let t = find_template("generic_resource_site").expect("generic_resource_site 模板存在");
        assert!(
            t.field_tree
                .list_page
                .iter()
                .any(|n| n.spec.name == "resource_card")
        );
        assert!(
            t.field_tree
                .detail_page
                .iter()
                .any(|n| n.spec.name == "title")
        );
        assert!(
            t.field_tree
                .detail_page
                .iter()
                .any(|n| n.spec.name == "content")
        );
    }
}
