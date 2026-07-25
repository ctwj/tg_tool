//! `crawler_field_library` 表的 DB 行模型（feature 043-crawler-configurator，T017）
//!
//! 预置字段库 — 启动时由 `services::crawler::preset_library` 种子化，
//! 用户在字段配置器中按 category 分组勾选插入到任务字段树。

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// DB 行：`crawler_field_library` 一行
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct FieldLibraryRow {
    pub id: i64,
    pub key: String,
    pub display_name: String,
    /// string/text/url/image/number/datetime/link_card/custom
    pub field_type: String,
    /// basic / metadata / classification / interaction / resource
    pub category: String,
    pub description: Option<String>,
    /// 建议的匹配模式：css / regex / prefix_suffix / json_path / meta_attr / header_field
    pub suggested_extractor: Option<String>,
    pub sort_order: i32,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// API 响应视图：按 category 分组
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldLibraryCategory {
    pub category: String,
    pub label: String,
    pub entries: Vec<FieldLibraryRow>,
}

/// category 中文标签
pub fn category_label(cat: &str) -> &'static str {
    match cat {
        "basic" => "基础",
        "metadata" => "元数据",
        "classification" => "分类",
        "interaction" => "互动指标",
        "resource" => "资源链接",
        _ => "其它",
    }
}

/// 把扁平 rows 组装为按 category 分组的视图
///
/// 排序：category 字典序 → sort_order → id
pub fn group_by_category(rows: Vec<FieldLibraryRow>) -> Vec<FieldLibraryCategory> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, Vec<FieldLibraryRow>> = BTreeMap::new();
    for r in rows {
        map.entry(r.category.clone()).or_default().push(r);
    }
    let mut out: Vec<FieldLibraryCategory> = map
        .into_iter()
        .map(|(cat, mut entries)| {
            entries.sort_by(|a, b| a.sort_order.cmp(&b.sort_order).then(a.id.cmp(&b.id)));
            let label = category_label(&cat).to_string();
            FieldLibraryCategory {
                category: cat,
                label,
                entries,
            }
        })
        .collect();
    // 固定顺序：basic → metadata → classification → interaction → resource → 其它
    out.sort_by_key(|c| category_order(&c.category));
    out
}

fn category_order(cat: &str) -> u8 {
    match cat {
        "basic" => 0,
        "metadata" => 1,
        "classification" => 2,
        "interaction" => 3,
        "resource" => 4,
        _ => 255,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: i64, key: &str, cat: &str, sort: i32) -> FieldLibraryRow {
        FieldLibraryRow {
            id,
            key: key.into(),
            display_name: key.into(),
            field_type: "string".into(),
            category: cat.into(),
            description: None,
            suggested_extractor: Some("css".into()),
            sort_order: sort,
            created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0)
                .unwrap()
                .naive_utc(),
            updated_at: chrono::DateTime::from_timestamp(1_700_000_000, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    #[test]
    fn category_label_known() {
        assert_eq!(category_label("basic"), "基础");
        assert_eq!(category_label("metadata"), "元数据");
        assert_eq!(category_label("classification"), "分类");
        assert_eq!(category_label("interaction"), "互动指标");
        assert_eq!(category_label("resource"), "资源链接");
        assert_eq!(category_label("unknown"), "其它");
    }

    #[test]
    fn group_by_category_preserves_fixed_order() {
        // 故意打乱输入顺序
        let rows = vec![
            entry(5, "e", "resource", 0),
            entry(1, "a", "basic", 0),
            entry(3, "c", "interaction", 0),
            entry(2, "b", "metadata", 0),
            entry(4, "d", "classification", 0),
        ];
        let groups = group_by_category(rows);
        let cats: Vec<_> = groups.iter().map(|g| g.category.as_str()).collect();
        assert_eq!(
            cats,
            vec![
                "basic",
                "metadata",
                "classification",
                "interaction",
                "resource"
            ]
        );
    }

    #[test]
    fn group_by_category_sorts_entries_within_group() {
        let rows = vec![
            entry(3, "c", "basic", 2),
            entry(1, "a", "basic", 0),
            entry(2, "b", "basic", 1),
        ];
        let groups = group_by_category(rows);
        assert_eq!(groups.len(), 1);
        let keys: Vec<_> = groups[0].entries.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, vec!["a", "b", "c"]);
    }

    #[test]
    fn group_by_category_unknown_cat_goes_last() {
        let rows = vec![entry(2, "z", "custom_cat", 0), entry(1, "a", "basic", 0)];
        let groups = group_by_category(rows);
        let cats: Vec<_> = groups.iter().map(|g| g.category.as_str()).collect();
        assert_eq!(cats, vec!["basic", "custom_cat"]);
    }

    #[test]
    fn group_by_category_empty_input() {
        let groups = group_by_category(vec![]);
        assert!(groups.is_empty());
    }
}
