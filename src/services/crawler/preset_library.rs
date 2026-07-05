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

/// 45 条预置字段（data-model.md E2 + 资源下载场景扩展 + 游戏/软件/教程场景扩展 + 视频场景扩展 + 站点 ID）
///
/// 顺序：basic(6) → metadata(7) → classification(5) → interaction(4) → resource(23)
/// metadata 7 条 = 原 6 条（author/published_at/updated_at/source_site/canonical_url/copyright）
///                + 站点 ID 1 条（id：站点资源 ID，跨页去重用）
/// resource 23 条 = 原 5 条（download_url/resource_name/file_size/duration/version）
///                 + 游戏/软件/教程扩展 11 条（platform/developer/publisher/release_date/
///                   system_requirements/format/license/instructor/lesson_count/
///                   course_duration/course_level）
///                 + 视频扩展 6 条（video_url/video_cover/video_duration/video_resolution/
///                   video_codec/subtitles）
///                 + APP 场景 1 条（app_icon）
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
    PresetField {
        key: "id",
        display_name: "ID",
        field_type: "string",
        category: "metadata",
        description: "站点资源 ID（如 discuz thread-12345 中的 12345 / wordpress post-12345 中的 12345，用于跨页去重和站内唯一标识）",
        suggested_extractor: "regex",
        sort_order: 7,
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
    // 资源属性 — 游戏/软件/教程场景扩展（sort_order 6~16）
    PresetField {
        key: "platform",
        display_name: "平台",
        field_type: "string",
        category: "resource",
        description: "运行平台（Windows/macOS/Linux/iOS/Android/PS5/Xbox/Switch 等），多个用分隔符或重复抓取",
        suggested_extractor: "css",
        sort_order: 6,
    },
    PresetField {
        key: "developer",
        display_name: "开发者",
        field_type: "string",
        category: "resource",
        description: "软件/游戏开发者（工作室/公司）",
        suggested_extractor: "css",
        sort_order: 7,
    },
    PresetField {
        key: "publisher",
        display_name: "发行商",
        field_type: "string",
        category: "resource",
        description: "软件/游戏发行商",
        suggested_extractor: "css",
        sort_order: 8,
    },
    PresetField {
        key: "release_date",
        display_name: "发布日期",
        field_type: "datetime",
        category: "resource",
        description: "资源正式发布/发售日期（游戏/软件/教程通用）",
        suggested_extractor: "css",
        sort_order: 9,
    },
    PresetField {
        key: "system_requirements",
        display_name: "系统要求",
        field_type: "text",
        category: "resource",
        description: "最低配置 / 推荐配置 / 兼容系统版本（游戏/软件）",
        suggested_extractor: "css",
        sort_order: 10,
    },
    PresetField {
        key: "format",
        display_name: "资源格式",
        field_type: "string",
        category: "resource",
        description: "文件格式（MP4/PDF/EXE/DMG/ISO/RAR/MKV 等），可用于后续资源类型分流",
        suggested_extractor: "regex",
        sort_order: 11,
    },
    PresetField {
        key: "license",
        display_name: "授权类型",
        field_type: "string",
        category: "resource",
        description: "免费/开源/付费/订阅/试用 等授权模型（软件类常用）",
        suggested_extractor: "css",
        sort_order: 12,
    },
    PresetField {
        key: "instructor",
        display_name: "讲师",
        field_type: "string",
        category: "resource",
        description: "教程讲师/作者（视频课程/付费教程场景）",
        suggested_extractor: "css",
        sort_order: 13,
    },
    PresetField {
        key: "lesson_count",
        display_name: "章节数",
        field_type: "number",
        category: "resource",
        description: "教程章节/课时数量",
        suggested_extractor: "regex",
        sort_order: 14,
    },
    PresetField {
        key: "course_duration",
        display_name: "课程时长",
        field_type: "string",
        category: "resource",
        description: "教程总时长（如 12小时30分 / 12:30:00）",
        suggested_extractor: "regex",
        sort_order: 15,
    },
    PresetField {
        key: "course_level",
        display_name: "难度等级",
        field_type: "string",
        category: "resource",
        description: "入门/初级/中级/高级/专家（教程类常用）",
        suggested_extractor: "css",
        sort_order: 16,
    },
    // 资源属性 — 视频场景扩展（sort_order 17~22）
    PresetField {
        key: "video_url",
        display_name: "视频地址",
        field_type: "url",
        category: "resource",
        description: "视频源地址（MP4/M3U8/Embed 等），区别于通用 download_url",
        suggested_extractor: "css",
        sort_order: 17,
    },
    PresetField {
        key: "video_cover",
        display_name: "视频封面",
        field_type: "image",
        category: "resource",
        description: "视频独立封面（详情页播放器封面，区别于列表卡片 cover）",
        suggested_extractor: "css",
        sort_order: 18,
    },
    PresetField {
        key: "video_duration",
        display_name: "视频时长",
        field_type: "string",
        category: "resource",
        description: "单条视频时长（如 12:30 / 12分30秒），区别于教程总时长 course_duration",
        suggested_extractor: "regex",
        sort_order: 19,
    },
    PresetField {
        key: "video_resolution",
        display_name: "清晰度",
        field_type: "string",
        category: "resource",
        description: "分辨率（720p / 1080p / 2K / 4K / 8K）",
        suggested_extractor: "regex",
        sort_order: 20,
    },
    PresetField {
        key: "video_codec",
        display_name: "视频编码",
        field_type: "string",
        category: "resource",
        description: "视频编码格式（H.264 / H.265 / AV1 / VP9）",
        suggested_extractor: "regex",
        sort_order: 21,
    },
    PresetField {
        key: "subtitles",
        display_name: "字幕",
        field_type: "string",
        category: "resource",
        description: "字幕语言（中字 / 英字 / 中英双语 / 内嵌字幕 / 外挂字幕）",
        suggested_extractor: "css",
        sort_order: 22,
    },
    // 资源属性 — APP 场景扩展（sort_order 23）
    PresetField {
        key: "app_icon",
        display_name: "APP 图标",
        field_type: "image",
        category: "resource",
        description: "软件 / APP 应用图标 URL（区别于通用 cover，常用于下载站、应用市场）",
        suggested_extractor: "css",
        sort_order: 23,
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
    fn t_builtin_presets_count_exactly_45() {
        // data-model.md E2 原始 24 条 + 资源场景扩展 2 条（download_url / resource_name）
        // + 游戏/软件/教程场景扩展 11 条（platform/developer/publisher/release_date/
        //   system_requirements/format/license/instructor/lesson_count/course_duration/course_level）
        // + 视频场景扩展 6 条（video_url/video_cover/video_duration/video_resolution/video_codec/subtitles）
        // + APP 场景扩展 1 条（app_icon）
        // + metadata 扩展 1 条（id：站点资源 ID）
        assert_eq!(BUILTIN_PRESETS.len(), 45);
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
        assert_eq!(resource.len(), 23, "resource 类应有 23 条（原 5 + 游戏/软件/教程 11 + 视频 6 + APP 1）");
        assert_eq!(resource[0].key, "download_url");
        assert_eq!(resource[1].key, "resource_name");
        assert_eq!(resource[0].field_type, "url");
        assert_eq!(resource[1].field_type, "string");
        // 原 3 条顺延到 3/4/5
        assert_eq!(resource[2].key, "file_size");
        assert_eq!(resource[3].key, "duration");
        assert_eq!(resource[4].key, "version");
        // 新增 11 条顺序
        assert_eq!(resource[5].key, "platform");
        assert_eq!(resource[6].key, "developer");
        assert_eq!(resource[7].key, "publisher");
        assert_eq!(resource[8].key, "release_date");
        assert_eq!(resource[9].key, "system_requirements");
        assert_eq!(resource[10].key, "format");
        assert_eq!(resource[11].key, "license");
        assert_eq!(resource[12].key, "instructor");
        assert_eq!(resource[13].key, "lesson_count");
        assert_eq!(resource[14].key, "course_duration");
        assert_eq!(resource[15].key, "course_level");
        // 视频扩展 6 条
        assert_eq!(resource[16].key, "video_url");
        assert_eq!(resource[17].key, "video_cover");
        assert_eq!(resource[18].key, "video_duration");
        assert_eq!(resource[19].key, "video_resolution");
        assert_eq!(resource[20].key, "video_codec");
        assert_eq!(resource[21].key, "subtitles");
        // APP 扩展 1 条
        assert_eq!(resource[22].key, "app_icon");
    }

    #[test]
    fn t_resource_new_fields_keys_unique_vs_existing() {
        // 新增 11 字段不能与原 26 字段 key 冲突（key 全局唯一约束）
        let new_keys = [
            "platform", "developer", "publisher", "release_date", "system_requirements",
            "format", "license", "instructor", "lesson_count", "course_duration", "course_level",
        ];
        let existing: std::collections::HashSet<&str> =
            BUILTIN_PRESETS.iter().map(|p| p.key).collect();
        for k in new_keys {
            assert!(existing.contains(k), "新增字段 {k} 未在 BUILTIN_PRESETS 中");
        }
        // category 全部归 resource
        for k in new_keys {
            let p = BUILTIN_PRESETS.iter().find(|p| p.key == k).unwrap();
            assert_eq!(p.category, "resource", "{} 应归类 resource", k);
        }
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
