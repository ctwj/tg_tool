-- 清理 extracted_resources 中 url 相同的重复记录，保留最早的一条
DELETE FROM extracted_resources
WHERE ctid NOT IN (
    SELECT MIN(ctid) FROM extracted_resources WHERE url IS NOT NULL AND url != '' GROUP BY url
);

-- 创建 url 唯一索引，防止未来重复插入
CREATE UNIQUE INDEX IF NOT EXISTS idx_extracted_resources_url
ON extracted_resources(url);
