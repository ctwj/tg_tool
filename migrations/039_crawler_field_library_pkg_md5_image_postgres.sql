-- 039: crawler_field_library 增量插入 pkg_name/md5/image 字段 (PostgreSQL) — feature 046 后续增强
-- 用途：在 resource 分类下新增 3 条安装包元信息相关预置字段
--   - pkg_name：应用包名（Android applicationId / iOS Bundle Identifier），跨站去重和版本比对用
--   - md5：安装包/资源文件 MD5 校验值，完整性校验和去重用
--   - image：通用图片字段 URL（详情页截图、宣传图、二维码等，区别于 cover/app_icon）
-- INSERT ... ON CONFLICT (key) DO NOTHING 保证幂等（与 038 风格一致）

INSERT INTO crawler_field_library
    (key, display_name, field_type, category, description, suggested_extractor, sort_order)
VALUES
    ('pkg_name', '包名', 'string', 'resource', '应用包名（Android applicationId / iOS Bundle Identifier，如 com.example.app），用于跨站去重和版本比对', 'regex', 24),
    ('md5', 'MD5 校验码', 'string', 'resource', '安装包/资源文件 MD5 校验值（32 位十六进制），用于完整性校验和去重', 'regex', 25),
    ('image', '图片', 'image', 'resource', '通用图片字段 URL（详情页截图、宣传图、二维码等，区别于 cover/app_icon 等专用图片字段）', 'css', 26)
ON CONFLICT (key) DO NOTHING;
