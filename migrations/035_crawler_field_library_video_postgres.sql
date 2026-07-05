-- 035: crawler_field_library resource 类扩展（视频场景 6 字段）(PostgreSQL)
-- 幂等：ON CONFLICT (key) DO NOTHING。新部署由 027 + preset_library::BUILTIN_PRESETS 直接写入，
-- 现有 DB（已跑过 027/031/034）需本迁移补齐。
--
-- 新增字段（统一 category='resource'，sort_order 续 17~22）：
--   video_url / video_cover / video_duration / video_resolution / video_codec / subtitles

INSERT INTO crawler_field_library
    (key, display_name, field_type, category, description, suggested_extractor, sort_order)
VALUES
    ('video_url',         '视频地址',  'url',    'resource', '视频源地址（MP4/M3U8/Embed 等），区别于通用 download_url',             'css',   17),
    ('video_cover',       '视频封面',  'image',  'resource', '视频独立封面（详情页播放器封面，区别于列表卡片 cover）',                'css',   18),
    ('video_duration',    '视频时长',  'string', 'resource', '单条视频时长（如 12:30 / 12分30秒），区别于教程总时长 course_duration', 'regex', 19),
    ('video_resolution',  '清晰度',    'string', 'resource', '分辨率（720p / 1080p / 2K / 4K / 8K）',                              'regex', 20),
    ('video_codec',       '视频编码',  'string', 'resource', '视频编码格式（H.264 / H.265 / AV1 / VP9）',                          'regex', 21),
    ('subtitles',         '字幕',      'string', 'resource', '字幕语言（中字 / 英字 / 中英双语 / 内嵌字幕 / 外挂字幕）',              'css',   22)
ON CONFLICT (key) DO NOTHING;
