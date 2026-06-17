-- Migration 018: forward_tasks 增加 (remote_id, id DESC) 复合索引
-- 修复资源列表分页慢：RESOURCE_COLS 含 3 个相关子查询按 remote_id 过滤 + id DESC 排序取一条
-- 无此索引时 1 万资源翻末页会全表扫描数十万次，30s+ 超时
CREATE INDEX IF NOT EXISTS idx_forward_tasks_remote_id
  ON forward_tasks(remote_id, id DESC);
