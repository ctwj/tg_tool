-- 036: crawler_field_library resource 类扩展（APP 图标）(PostgreSQL)
-- 幂等：ON CONFLICT (key) DO NOTHING。新部署由 027 + preset_library::BUILTIN_PRESETS 直接写入，
-- 现有 DB（已跑过 027/031/034/035）需本迁移补齐。
--
-- 新增字段（统一 category='resource'，sort_order 续 23）：
--   app_icon

INSERT INTO crawler_field_library
    (key, display_name, field_type, category, description, suggested_extractor, sort_order)
VALUES
    ('app_icon', 'APP 图标', 'image', 'resource',
     '软件 / APP 应用图标 URL（区别于通用 cover，常用于下载站、应用市场）',
     'css', 23)
ON CONFLICT (key) DO NOTHING;
