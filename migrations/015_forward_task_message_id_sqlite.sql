-- 015: forward_tasks 加 image_message_id 字段（双群组两阶段转存）
-- 阶段1（客户端 copy_media 转存到群组A）成功后写入群组A 中的消息 ID
-- 用于阶段2 Bot forwardMessage 调用，NULL 表示阶段1 未完成
ALTER TABLE forward_tasks ADD COLUMN image_message_id INTEGER;

-- 部分索引：调度器每 2 秒查 awaiting_bot 任务时受益
CREATE INDEX IF NOT EXISTS idx_forward_tasks_awaiting
  ON forward_tasks(status) WHERE status = 'awaiting_bot';
