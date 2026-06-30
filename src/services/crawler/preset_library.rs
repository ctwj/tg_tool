//! 预置字段库（feature 043-crawler-configurator）
//!
//! 与 migration 027 内嵌的种子数据互为兜底：SQL INSERT 是第一道种子，
//! 本模块 `seed_if_empty_sqlite` / `seed_if_empty_postgres` 是应用层兜底
//! （防止 SQL 种子因 schema drift 或迁移幂等性判断被跳过时表为空）。
//!
//! 与 data-model.md E2 种子表保持一致。

use sqlx::PgPool;
use sqlx::SqlitePool;

/// 一条预置字段定义
#[derive(Debug, Clone)]
pub struct PresetField {
    pub key: &'static str,
    pub display_name: &'static str,
    pub field_type: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub suggested_extractor: &'static str,
    pub sort_order: i32,
}

/// 26 条预置字段（data-model.md E2 + 资源下载场景扩展）
///
/// 顺序：basic(6) → metadata(6) → classification(5) → interaction(4) → resource(5)
pub static BUILTIN_PRESETS: &[PresetField] = &[
    // 基础字段
    PresetField {
        key: "title",
        display_name: "标题",
        field_type: "string",
        category: "basic",
        description: "文章标题",
        suggested_extractor: "css",
        sort_order: 1,
    },
    PresetField {
        key: "url",
        display_name: "链接",
        field_type: "url",
        category: "basic",
        description: "详情页链接",
        suggested_extractor: "css",
        sort_order: 2,
    },
    PresetField {
        key: "cover",
        display_name: "封面",
        field_type: "image",
        category: "basic",
        description: "列表卡片封面图",
        suggested_extractor: "css",
        sort_order: 3,
    },
    PresetField {
        key: "thumbnail",
        display_name: "缩略图",
        field_type: "image",
        category: "basic",
        description: "小尺寸预览图",
        suggested_extractor: "css",
        sort_order: 4,
    },
    PresetField {
        key: "description",
        display_name: "描述",
        field_type: "text",
        category: "basic",
        description: "摘要/副标题/简介",
        suggested_extractor: "css",
        sort_order: 5,
    },
    PresetField {
        key: "content",
        display_name: "正文",
        field_type: "text",
        category: "basic",
        description: "详情页正文 HTML",
        suggested_extractor: "css",
        sort_order: 6,
    },
    // 元数据
    PresetField {
        key: "author",
        display_name: "作者",
        field_type: "string",
        category: "metadata",
        description: "文章作者",
        suggested_extractor: "css",
        sort_order: 1,
    },
    PresetField {
        key: "published_at",
        display_name: "发布时间",
        field_type: "datetime",
        category: "metadata",
        description: "发布时间",
        suggested_extractor: "css",
        sort_order: 2,
    },
    PresetField {
        key: "updated_at",
        display_name: "更新时间",
        field_type: "datetime",
        category: "metadata",
        description: "最后更新时间",
        suggested_extractor: "css",
        sort_order: 3,
    },
    PresetField {
        key: "source_site",
        display_name: "来源站点",
        field_type: "string",
        category: "metadata",
        description: "来源站点名",
        suggested_extractor: "meta_attr",
        sort_order: 4,
    },
    PresetField {
        key: "canonical_url",
        display_name: "原文链接",
        field_type: "url",
        category: "metadata",
        description: "canonical 链接",
        suggested_extractor: "meta_attr",
        sort_order: 5,
    },
    PresetField {
        key: "copyright",
        display_name: "版权声明",
        field_type: "string",
        category: "metadata",
        description: "版权声明文本",
        suggested_extractor: "css",
        sort_order: 6,
    },
    // 分类与标签
    PresetField {
        key: "category",
        display_name: "分类",
        field_type: "string",
        category: "classification",
        description: "文章分类",
        suggested_extractor: "css",
        sort_order: 1,
    },
    PresetField {
        key: "tags",
        display_name: "标签",
        field_type: "string",
        category: "classification",
        description: "标签列表",
        suggested_extractor: "css",
        sort_order: 2,
    },
    PresetField {
        key: "content_type",
        display_name: "内容类型",
        field_type: "string",
        category: "classification",
        description: "如视频/文章/资源",
        suggested_extractor: "meta_attr",
        sort_order: 3,
    },
    PresetField {
        key: "language",
        display_name: "语言",
        field_type: "string",
        category: "classification",
        description: "内容语言",
        suggested_extractor: "meta_attr",
        sort_order: 4,
    },
    PresetField {
        key: "region",
        display_name: "地区",
        field_type: "string",
        category: "classification",
        description: "内容地区",
        suggested_extractor: "meta_attr",
        sort_order: 5,
    },
    // 互动指标
    PresetField {
        key: "view_count",
        display_name: "浏览量",
        field_type: "number",
        category: "interaction",
        description: "浏览次数",
        suggested_extractor: "regex",
        sort_order: 1,
    },
    PresetField {
        key: "comment_count",
        display_name: "评论数",
        field_type: "number",
        category: "interaction",
        description: "评论条数",
        suggested_extractor: "regex",
        sort_order: 2,
    },
    PresetField {
        key: "like_count",
        display_name: "点赞数",
        field_type: "number",
        category: "interaction",
        description: "点赞次数",
        suggested_extractor: "regex",
        sort_order: 3,
    },
    PresetField {
        key: "rating",
        display_name: "评分",
        field_type: "number",
        category: "interaction",
        description: "评分（5 分制）",
        suggested_extractor: "regex",
        sort_order: 4,
    },
    // 资源属性（download_url / resource_name 是资源类核心，排最前）
    PresetField {
        key: "download_url",
        display_name: "下载地址",
        field_type: "url",
        category: "resource",
        description: "资源下载地址（直链或网盘）。可能需用 follow_url 模式两阶段提取",
        suggested_extractor: "css",
        sort_order: 1,
    },
    PresetField {
        key: "resource_name",
        display_name: "资源名",
        field_type: "string",
        category: "resource",
        description: "资源名称（区别于文章标题 title，适用于一篇文章列多个资源的场景）",
        suggested_extractor: "css",
        sort_order: 2,
    },
    PresetField {
        key: "file_size",
        display_name: "附件大小",
        field_type: "string",
        category: "resource",
        description: "附件文件大小",
        suggested_extractor: "regex",
        sort_order: 3,
    },
    PresetField {
        key: "duration",
        display_name: "时长",
        field_type: "string",
        category: "resource",
        description: "音视频时长",
        suggested_extractor: "regex",
        sort_order: 4,
    },
    PresetField {
        key: "version",
        display_name: "版本号",
        field_type: "string",
        category: "resource",
        description: "软件/资源版本号",
        suggested_extractor: "regex",
        sort_order: 5,
    },
];

/// SQLite 兜底种子化：若 `crawler_field_library` 为空则批量 INSERT OR IGNORE
pub async fn seed_if_empty_sqlite(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM crawler_field_library")
        .fetch_one(pool)
        .await?;
    if count > 0 {
        tracing::debug!("crawler_field_library 已有 {count} 条，跳过种子化");
        return Ok(());
    }
    tracing::info!("crawler_field_library 为空，开始应用层种子化（{} 条）", BUILTIN_PRESETS.len());
    for p in BUILTIN_PRESETS {
        sqlx::query(
            "INSERT OR IGNORE INTO crawler_field_library \
             (key, display_name, field_type, category, description, suggested_extractor, sort_order) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(p.key)
        .bind(p.display_name)
        .bind(p.field_type)
        .bind(p.category)
        .bind(p.description)
        .bind(p.suggested_extractor)
        .bind(p.sort_order)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// PostgreSQL 兜底种子化：若 `crawler_field_library` 为空则批量 INSERT ON CONFLICT DO NOTHING
pub async fn seed_if_empty_postgres(pool: &PgPool) -> Result<(), sqlx::Error> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM crawler_field_library")
        .fetch_one(pool)
        .await?;
    if count > 0 {
        tracing::debug!("crawler_field_library 已有 {count} 条，跳过种子化");
        return Ok(());
    }
    tracing::info!("crawler_field_library 为空，开始应用层种子化（{} 条）", BUILTIN_PRESETS.len());
    for p in BUILTIN_PRESETS {
        sqlx::query(
            "INSERT INTO crawler_field_library \
             (key, display_name, field_type, category, description, suggested_extractor, sort_order) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (key) DO NOTHING",
        )
        .bind(p.key)
        .bind(p.display_name)
        .bind(p.field_type)
        .bind(p.category)
        .bind(p.description)
        .bind(p.suggested_extractor)
        .bind(p.sort_order)
        .execute(pool)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_builtin_presets_at_least_20() {
        assert!(
            BUILTIN_PRESETS.len() >= 20,
            "BUILTIN_PRESETS 至少 20 条，实际 {}",
            BUILTIN_PRESETS.len()
        );
    }

    #[test]
    fn t_builtin_presets_count_exactly_26() {
        // data-model.md E2 原始 24 条 + 资源场景扩展 2 条（download_url / resource_name）
        assert_eq!(BUILTIN_PRESETS.len(), 26);
    }

    #[test]
    fn t_builtin_presets_keys_unique() {
        let mut keys: Vec<&str> = BUILTIN_PRESETS.iter().map(|p| p.key).collect();
        keys.sort();
        let total = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), total, "存在重复 key");
    }

    #[test]
    fn t_builtin_presets_categories_cover_5() {
        let cats: std::collections::HashSet<&str> =
            BUILTIN_PRESETS.iter().map(|p| p.category).collect();
        assert!(cats.contains("basic"));
        assert!(cats.contains("metadata"));
        assert!(cats.contains("classification"));
        assert!(cats.contains("interaction"));
        assert!(cats.contains("resource"));
    }

    #[test]
    fn t_builtin_presets_required_keys_present() {
        let keys: std::collections::HashSet<&str> =
            BUILTIN_PRESETS.iter().map(|p| p.key).collect();
        for required in [
            "title", "url", "cover", "description", "content", "author", "published_at",
            "category", "tags", "view_count", "file_size", "download_url", "resource_name",
        ] {
            assert!(keys.contains(required), "缺失必需字段 {required}");
        }
    }

    #[test]
    fn t_resource_category_sort_order_download_url_first() {
        // download_url 必须排第 1，resource_name 排第 2（资源场景核心字段）
        let mut resource: Vec<&PresetField> =
            BUILTIN_PRESETS.iter().filter(|p| p.category == "resource").collect();
        resource.sort_by_key(|p| p.sort_order);
        assert_eq!(resource[0].key, "download_url");
        assert_eq!(resource[1].key, "resource_name");
        assert_eq!(resource[0].field_type, "url");
        assert_eq!(resource[1].field_type, "string");
        // 原 3 条顺延到 3/4/5
        assert_eq!(resource[2].key, "file_size");
        assert_eq!(resource[3].key, "duration");
        assert_eq!(resource[4].key, "version");
    }

    #[test]
    fn t_suggested_extractor_valid() {
        let valid = [
            "css",
            "regex",
            "prefix_suffix",
            "json_path",
            "meta_attr",
            "header_field",
        ];
        for p in BUILTIN_PRESETS {
            assert!(
                valid.contains(&p.suggested_extractor),
                "{} 的 suggested_extractor={} 不合法",
                p.key,
                p.suggested_extractor
            );
        }
    }
}
