//! `crawler_task_field_nodes` 表的 DB 行模型（feature 043-crawler-configurator，T016）
//!
//! 与 `services::crawler::field_schema::FieldNodeSpec` 的关系：
//! - 本文件 = DB 行层（`rule_json` 是未解析的 JSON 字符串）
//! - `field_schema::FieldNodeSpec` = 应用层（`rule: Rule` 已解析为具体变体）
//! - 通过 `to_spec()` 互转；handlers 接收应用层 spec，落库时序列化为 rule_json

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::services::crawler::field_schema::{
    ExtractorMode, FieldType, PostProcessor, Rule, Scope, SourceLayer,
};

/// DB 行：`crawler_task_field_nodes` 一行
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct FieldNodeRow {
    pub id: i64,
    pub task_id: i64,
    pub parent_id: Option<i64>,
    /// "list_page" | "detail_page"
    pub scope: String,
    pub name: String,
    pub display_name: String,
    /// string/text/url/image/number/datetime/link_card/custom
    pub field_type: String,
    /// html/header/script/meta/url
    pub source_layer: String,
    /// css/regex/prefix_suffix/json_path/meta_attr/header_field
    pub extractor_mode: String,
    /// 模式特定规则的 JSON 字符串
    pub rule_json: String,
    /// 后处理链 JSON 数组，可空
    pub post_processors_json: Option<String>,
    /// source_layer=script 时指定的脚本块索引
    pub script_index: Option<i32>,
    pub sort_order: i32,
    pub is_active: bool,
    /// [feature 046] 仅 extractor_mode=script 时允许 true；其它模式必须 false
    #[sqlx(default)]
    pub refresh_on_read: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl FieldNodeRow {
    /// 把 row 转为应用层 spec（解析 rule_json、post_processors_json）
    ///
    /// 失败场景：DB 中存在非法 rule_json（手工 SQL 改坏）。
    /// 失败时返回错误字符串，由调用方决定降级策略。
    pub fn to_spec(&self) -> Result<FieldNodeSpecView, String> {
        let scope =
            Scope::from_str(&self.scope).ok_or_else(|| format!("非法 scope: {}", self.scope))?;
        let field_type = FieldType::from_str(&self.field_type)
            .ok_or_else(|| format!("非法 field_type: {}", self.field_type))?;
        let source_layer = SourceLayer::from_str(&self.source_layer)
            .ok_or_else(|| format!("非法 source_layer: {}", self.source_layer))?;
        let extractor_mode = ExtractorMode::from_str(&self.extractor_mode)
            .ok_or_else(|| format!("非法 extractor_mode: {}", self.extractor_mode))?;

        let rule = crate::services::crawler::field_schema::deserialize_rule(
            extractor_mode,
            &self.rule_json,
        )?;

        let post_processors: Vec<PostProcessor> = match &self.post_processors_json {
            None => Vec::new(),
            Some(s) if s.trim().is_empty() => Vec::new(),
            Some(s) => serde_json::from_str(s)
                .map_err(|e| format!("post_processors_json 解析失败: {e}"))?,
        };

        Ok(FieldNodeSpecView {
            id: Some(self.id),
            task_id: Some(self.task_id),
            parent_id: self.parent_id,
            scope,
            name: self.name.clone(),
            display_name: self.display_name.clone(),
            field_type,
            source_layer,
            extractor_mode,
            rule,
            post_processors,
            script_index: self.script_index,
            sort_order: self.sort_order,
            is_active: self.is_active,
            refresh_on_read: self.refresh_on_read,
        })
    }
}

/// 应用层视图（与 `field_schema::FieldNodeSpec` 字段一致，但作为 model 层独立类型）
///
/// 与 `FieldNodeRow` 的差异：rule/post_processors 已解析为具体类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldNodeSpecView {
    pub id: Option<i64>,
    pub task_id: Option<i64>,
    pub parent_id: Option<i64>,
    pub scope: Scope,
    pub name: String,
    pub display_name: String,
    pub field_type: FieldType,
    pub source_layer: SourceLayer,
    pub extractor_mode: ExtractorMode,
    pub rule: Rule,
    #[serde(default)]
    pub post_processors: Vec<PostProcessor>,
    #[serde(default)]
    pub script_index: Option<i32>,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default = "default_true")]
    pub is_active: bool,
    /// [feature 046] 仅 extractor_mode=script 时允许 true
    #[serde(default)]
    pub refresh_on_read: bool,
}

fn default_true() -> bool {
    true
}

impl FieldNodeSpecView {
    /// 序列化为 DB 行所需的两段 JSON（rule_json + post_processors_json）
    ///
    /// 返回 `(rule_json, post_processors_json)`：
    /// - `rule_json` 是裸的 `{...}`（不含 mode 标签，mode 单独入 extractor_mode 列）
    /// - `post_processors_json` 为 None 表示空链
    pub fn to_db_json(&self) -> (String, Option<String>) {
        use crate::services::crawler::field_schema::serialize_rule;
        let (mode_str, rule_inner_json) = serialize_rule(&self.rule);
        debug_assert_eq!(mode_str, self.extractor_mode.as_str());
        let pp_json = if self.post_processors.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&self.post_processors).unwrap_or_else(|_| "[]".to_string()))
        };
        (rule_inner_json, pp_json)
    }
}

/// 字段树（应用层组装）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FieldTree {
    pub list_page: Vec<FieldTreeNode>,
    pub detail_page: Vec<FieldTreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldTreeNode {
    pub row: FieldNodeRow,
    pub children: Vec<FieldTreeNode>,
}

impl FieldTreeNode {
    /// 递归统计该子树节点总数（含自身）
    pub fn subtree_size(&self) -> usize {
        1 + self
            .children
            .iter()
            .map(FieldTreeNode::subtree_size)
            .sum::<usize>()
    }
}

/// 从扁平 rows 组装 FieldTree：
/// - 按 scope 分两组（list_page / detail_page）
/// - 每组按 parent_id 递归挂 children
/// - 同 parent 下按 sort_order、再按 id 排序
///
/// 容错：parent_id 指向的 row 不在集合内（已被删）→ 视作顶层
pub fn from_rows(rows: Vec<FieldNodeRow>) -> FieldTree {
    use std::collections::HashMap;

    // 按 id 索引（用于查找 parent 是否存在）
    let id_set: std::collections::HashSet<i64> = rows.iter().map(|r| r.id).collect();

    // 按 (parent_id, scope) 分桶
    // parent_id 用 0 作为"顶层"键（DB parent_id=NULL → 用 0 表示；parent_id 不在 id_set → 也归 0）
    let mut buckets: HashMap<(i64, String), Vec<FieldNodeRow>> = HashMap::new();
    for r in rows {
        let pkey = match r.parent_id {
            None => 0,
            Some(pid) if id_set.contains(&pid) => pid,
            _ => 0,
        };
        buckets.entry((pkey, r.scope.clone())).or_default().push(r);
    }

    // 排序每组
    for v in buckets.values_mut() {
        v.sort_by(|a, b| a.sort_order.cmp(&b.sort_order).then(a.id.cmp(&b.id)));
    }

    let list_page = build_children(0, "list_page", &mut buckets);
    let detail_page = build_children(0, "detail_page", &mut buckets);

    FieldTree {
        list_page,
        detail_page,
    }
}

/// 递归组装子节点：从 buckets 中取出 (parent_id, scope) 桶，遍历构造嵌套树
fn build_children(
    parent_id: i64,
    scope: &str,
    buckets: &mut std::collections::HashMap<(i64, String), Vec<FieldNodeRow>>,
) -> Vec<FieldTreeNode> {
    let key = (parent_id, scope.to_string());
    let parent_rows = match buckets.remove(&key) {
        Some(v) => v,
        None => return Vec::new(),
    };
    parent_rows
        .into_iter()
        .map(|r| {
            let rid = r.id;
            let rscope = r.scope.clone();
            let children = build_children(rid, &rscope, buckets);
            FieldTreeNode { row: r, children }
        })
        .collect()
}

/// 树形统计辅助：返回该树总节点数（含跨 scope）
pub fn tree_total_nodes(tree: &FieldTree) -> usize {
    tree.list_page
        .iter()
        .map(FieldTreeNode::subtree_size)
        .sum::<usize>()
        + tree
            .detail_page
            .iter()
            .map(FieldTreeNode::subtree_size)
            .sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        id: i64,
        task: i64,
        parent: Option<i64>,
        scope: &str,
        name: &str,
        sort: i32,
    ) -> FieldNodeRow {
        FieldNodeRow {
            id,
            task_id: task,
            parent_id: parent,
            scope: scope.into(),
            name: name.into(),
            display_name: name.into(),
            field_type: "string".into(),
            source_layer: "html".into(),
            extractor_mode: "css".into(),
            rule_json: r#"{"selector":".x","attr":"text"}"#.into(),
            post_processors_json: None,
            script_index: None,
            sort_order: sort,
            is_active: true,
            refresh_on_read: false,
            created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0)
                .unwrap()
                .naive_utc(),
            updated_at: chrono::DateTime::from_timestamp(1_700_000_000, 0)
                .unwrap()
                .naive_utc(),
        }
    }

    #[test]
    fn from_rows_flat() {
        let rows = vec![
            row(1, 100, None, "list_page", "title", 0),
            row(2, 100, None, "list_page", "url", 1),
            row(3, 100, None, "detail_page", "content", 0),
        ];
        let tree = from_rows(rows);
        assert_eq!(tree.list_page.len(), 2);
        assert_eq!(tree.detail_page.len(), 1);
        assert_eq!(tree.list_page[0].row.name, "title");
        assert_eq!(tree.list_page[1].row.name, "url");
        assert_eq!(tree.detail_page[0].row.name, "content");
    }

    #[test]
    fn from_rows_nested() {
        let rows = vec![
            row(1, 100, None, "list_page", "link_card", 0),
            row(2, 100, Some(1), "list_page", "title", 0),
            row(3, 100, Some(1), "list_page", "cover", 1),
            row(4, 100, Some(2), "list_page", "grandchild", 0), // 3 层
        ];
        let tree = from_rows(rows);
        assert_eq!(tree.list_page.len(), 1);
        let root = &tree.list_page[0];
        assert_eq!(root.row.id, 1);
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[0].row.id, 2);
        assert_eq!(root.children[0].children.len(), 1);
        assert_eq!(root.children[0].children[0].row.id, 4);
        assert_eq!(root.children[1].row.id, 3);
    }

    #[test]
    fn from_rows_sort_order_respected() {
        let rows = vec![
            row(3, 100, None, "list_page", "c", 2),
            row(1, 100, None, "list_page", "a", 0),
            row(2, 100, None, "list_page", "b", 1),
        ];
        let tree = from_rows(rows);
        let names: Vec<_> = tree.list_page.iter().map(|n| n.row.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn from_rows_dangling_parent_treated_as_top() {
        // parent_id 指向的 id 不在集合中 → 视作顶层
        let rows = vec![row(1, 100, Some(999), "list_page", "orphan", 0)];
        let tree = from_rows(rows);
        assert_eq!(tree.list_page.len(), 1);
        assert!(tree.list_page[0].children.is_empty());
    }

    #[test]
    fn subtree_size_counts_recursively() {
        let root = FieldTreeNode {
            row: row(1, 1, None, "list_page", "r", 0),
            children: vec![
                FieldTreeNode {
                    row: row(2, 1, Some(1), "list_page", "c1", 0),
                    children: vec![FieldTreeNode {
                        row: row(4, 1, Some(2), "list_page", "g", 0),
                        children: vec![],
                    }],
                },
                FieldTreeNode {
                    row: row(3, 1, Some(1), "list_page", "c2", 1),
                    children: vec![],
                },
            ],
        };
        assert_eq!(root.subtree_size(), 4);
    }

    #[test]
    fn tree_total_nodes_sums_both_scopes() {
        let rows = vec![
            row(1, 1, None, "list_page", "a", 0),
            row(2, 1, None, "detail_page", "b", 0),
            row(3, 1, Some(2), "detail_page", "c", 0),
        ];
        let tree = from_rows(rows);
        assert_eq!(tree_total_nodes(&tree), 3);
    }

    #[test]
    fn row_to_spec_roundtrip_css() {
        let r = row(1, 100, None, "list_page", "title", 0);
        let spec = r.to_spec().expect("css rule 解析成功");
        assert_eq!(spec.name, "title");
        assert_eq!(spec.scope, Scope::ListPage);
        assert_eq!(spec.field_type, FieldType::String);
        assert_eq!(spec.source_layer, SourceLayer::Html);
        assert_eq!(spec.extractor_mode, ExtractorMode::Css);
        assert!(spec.post_processors.is_empty());
    }

    #[test]
    fn row_to_spec_invalid_scope_fails() {
        let mut r = row(1, 100, None, "list_page", "title", 0);
        r.scope = "garbage".into();
        assert!(r.to_spec().is_err());
    }

    #[test]
    fn row_to_spec_invalid_rule_json_fails() {
        let mut r = row(1, 100, None, "list_page", "title", 0);
        r.rule_json = "not json".into();
        assert!(r.to_spec().is_err());
    }

    #[test]
    fn row_to_spec_parses_post_processors() {
        let mut r = row(1, 100, None, "list_page", "title", 0);
        r.post_processors_json = Some(r#"[{"op":"trim"},{"op":"absolutize_url"}]"#.into());
        let spec = r.to_spec().expect("解析成功");
        assert_eq!(spec.post_processors.len(), 2);
        // PostProcessorOp 序列化为 snake_case，验证其序列化结果即可
        let pp0_json = serde_json::to_string(&spec.post_processors[0].op).unwrap();
        assert_eq!(pp0_json, "\"trim\"");
        let pp1_json = serde_json::to_string(&spec.post_processors[1].op).unwrap();
        assert_eq!(pp1_json, "\"absolutize_url\"");
    }

    #[test]
    fn row_to_spec_empty_post_processors_treated_as_empty() {
        let mut r = row(1, 100, None, "list_page", "title", 0);
        r.post_processors_json = Some("".into());
        let spec = r.to_spec().expect("空字符串视为空数组");
        assert!(spec.post_processors.is_empty());
    }

    #[test]
    fn spec_to_db_json_roundtrip() {
        let r = row(1, 100, None, "list_page", "title", 0);
        let spec = r.to_spec().expect("解析");
        let (rule_json, pp_json) = spec.to_db_json();
        // rule_json 应能再次被解析
        assert!(
            crate::services::crawler::field_schema::deserialize_rule(
                spec.extractor_mode,
                &rule_json,
            )
            .is_ok()
        );
        assert!(pp_json.is_none()); // 空后处理链
    }
}
