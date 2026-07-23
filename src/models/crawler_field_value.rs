//! `crawler_article_field_values` 表的 DB 行模型（feature 043-crawler-configurator，T018）
//!
//! 抓取落库时长表 — 每个 (article, field_node) 对产生一行；
//! 多值字段用 value_index 区分；未命中字段也写入一行（is_hit=0）用于统计。

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// DB 行：`crawler_article_field_values` 一行
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ArticleFieldValueRow {
    pub id: i64,
    pub article_id: i64,
    /// 关联到 crawler_task_field_nodes.id；SET NULL：字段节点被删后历史值仍按 field_path 定位
    pub field_node_id: Option<i64>,
    /// 物化路径：/list_page/link_card/cover，便于 field_node 被删后仍可定位
    pub field_path: String,
    /// "list_page" | "detail_page"
    pub scope: String,
    /// 多值字段的索引（0=第一个）
    pub value_index: i32,
    /// 字符串值
    pub value_text: Option<String>,
    /// 数值字段（view_count/rating 等）解析后存此列便于聚合
    pub value_number: Option<f64>,
    /// false=该字段在该文章上未命中（FR-027 统计用）
    pub is_hit: bool,
    pub created_at: NaiveDateTime,
}

/// 字段统计（聚合视图）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FieldHitStats {
    pub field_path: String,
    pub total: u64,
    pub hit: u64,
    pub missed: u64,
}

impl FieldHitStats {
    pub fn hit_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.hit as f64 / self.total as f64
        }
    }
}

/// 从扁平 rows 按 field_path 聚合统计
pub fn aggregate_stats(rows: &[ArticleFieldValueRow]) -> Vec<FieldHitStats> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for r in rows {
        let entry = map.entry(r.field_path.clone()).or_insert((0, 0));
        entry.0 += 1; // total
        if r.is_hit {
            entry.1 += 1; // hit
        }
    }
    map.into_iter()
        .map(|(field_path, (total, hit))| FieldHitStats {
            field_path,
            total,
            hit,
            missed: total - hit,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value_row(id: i64, path: &str, is_hit: bool) -> ArticleFieldValueRow {
        ArticleFieldValueRow {
            id,
            article_id: 1,
            field_node_id: Some(10),
            field_path: path.into(),
            scope: "list_page".into(),
            value_index: 0,
            value_text: if is_hit { Some("v".into()) } else { None },
            value_number: None,
            is_hit,
            created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().naive_utc(),
        }
    }

    #[test]
    fn hit_rate_zero_total() {
        let s = FieldHitStats { field_path: "x".into(), total: 0, hit: 0, missed: 0 };
        assert_eq!(s.hit_rate(), 0.0);
    }

    #[test]
    fn hit_rate_normal() {
        let s = FieldHitStats { field_path: "x".into(), total: 4, hit: 3, missed: 1 };
        assert!((s.hit_rate() - 0.75).abs() < 1e-9);
    }

    #[test]
    fn aggregate_single_path() {
        let rows = vec![
            value_row(1, "/list_page/title", true),
            value_row(2, "/list_page/title", true),
            value_row(3, "/list_page/title", false), // miss
        ];
        let stats = aggregate_stats(&rows);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].field_path, "/list_page/title");
        assert_eq!(stats[0].total, 3);
        assert_eq!(stats[0].hit, 2);
        assert_eq!(stats[0].missed, 1);
    }

    #[test]
    fn aggregate_multiple_paths_sorted() {
        let rows = vec![
            value_row(1, "/list_page/cover", true),
            value_row(2, "/list_page/title", true),
            value_row(3, "/list_page/cover", false),
        ];
        let stats = aggregate_stats(&rows);
        let paths: Vec<_> = stats.iter().map(|s| s.field_path.as_str()).collect();
        // BTreeMap 自动按字典序
        assert_eq!(paths, vec!["/list_page/cover", "/list_page/title"]);
    }

    #[test]
    fn aggregate_all_miss() {
        let rows = vec![
            value_row(1, "/list_page/missing", false),
            value_row(2, "/list_page/missing", false),
        ];
        let stats = aggregate_stats(&rows);
        assert_eq!(stats[0].hit, 0);
        assert_eq!(stats[0].missed, 2);
        assert!((stats[0].hit_rate() - 0.0).abs() < 1e-9);
    }

    #[test]
    fn aggregate_empty() {
        let stats = aggregate_stats(&[]);
        assert!(stats.is_empty());
    }
}
