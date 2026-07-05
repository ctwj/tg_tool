-- 034: crawler_field_library resource 类扩展（游戏/软件/教程场景 11 字段）(SQLite)
-- 幂等：INSERT OR IGNORE，已存在则跳过。新部署由 027 + preset_library::BUILTIN_PRESETS 直接写入，
-- 但现有 DB（已跑过 027/031）需本迁移补齐。
--
-- 新增字段（统一 category='resource'，sort_order 续 6~16）：
--   platform / developer / publisher / release_date / system_requirements /
--   format / license / instructor / lesson_count / course_duration / course_level

INSERT OR IGNORE INTO crawler_field_library
    (key, display_name, field_type, category, description, suggested_extractor, sort_order)
VALUES
    ('platform',            '平台',       'string',   'resource', '运行平台（Windows/macOS/Linux/iOS/Android/PS5/Xbox/Switch 等），多个用分隔符或重复抓取', 'css',   6),
    ('developer',           '开发者',     'string',   'resource', '软件/游戏开发者（工作室/公司）',                                     'css',   7),
    ('publisher',           '发行商',     'string',   'resource', '软件/游戏发行商',                                                  'css',   8),
    ('release_date',        '发布日期',   'datetime', 'resource', '资源正式发布/发售日期（游戏/软件/教程通用）',                         'css',   9),
    ('system_requirements', '系统要求',   'text',     'resource', '最低配置 / 推荐配置 / 兼容系统版本（游戏/软件）',                      'css',   10),
    ('format',              '资源格式',   'string',   'resource', '文件格式（MP4/PDF/EXE/DMG/ISO/RAR/MKV 等），可用于后续资源类型分流',     'regex', 11),
    ('license',             '授权类型',   'string',   'resource', '免费/开源/付费/订阅/试用 等授权模型（软件类常用）',                     'css',   12),
    ('instructor',          '讲师',       'string',   'resource', '教程讲师/作者（视频课程/付费教程场景）',                                'css',   13),
    ('lesson_count',        '章节数',     'number',   'resource', '教程章节/课时数量',                                                 'regex', 14),
    ('course_duration',     '课程时长',   'string',   'resource', '教程总时长（如 12小时30分 / 12:30:00）',                              'regex', 15),
    ('course_level',        '难度等级',   'string',   'resource', '入门/初级/中级/高级/专家（教程类常用）',                                'css',   16);
