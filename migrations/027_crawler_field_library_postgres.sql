-- 027: 预置字段库表 crawler_field_library (PostgreSQL) — feature 043-crawler-configurator

CREATE TABLE IF NOT EXISTS crawler_field_library (
    id BIGSERIAL PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    field_type TEXT NOT NULL,
    category TEXT NOT NULL,
    description TEXT,
    suggested_extractor TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_field_library_category ON crawler_field_library(category, sort_order);

-- 种子数据：与应用层 preset_library.rs::BUILTIN_PRESETS 对齐
INSERT INTO crawler_field_library (key, display_name, field_type, category, description, suggested_extractor, sort_order) VALUES
    ('title',        '标题',     'string', 'basic', '文章标题',                   'css',     1),
    ('url',          '链接',     'url',    'basic', '详情页链接',                 'css',     2),
    ('cover',        '封面',     'image',  'basic', '列表卡片封面图',             'css',     3),
    ('thumbnail',    '缩略图',   'image',  'basic', '小尺寸预览图',               'css',     4),
    ('description',  '描述',     'text',   'basic', '摘要/副标题/简介',           'css',     5),
    ('content',      '正文',     'text',   'basic', '详情页正文 HTML',            'css',     6),
    ('author',        '作者',     'string',   'metadata', '文章作者',           'css',      7),
    ('published_at',  '发布时间', 'datetime', 'metadata', '发布时间',           'css',      8),
    ('updated_at',    '更新时间', 'datetime', 'metadata', '最后更新时间',       'css',      9),
    ('source_site',   '来源站点', 'string',   'metadata', '来源站点名',         'meta_attr',10),
    ('canonical_url', '原文链接', 'url',      'metadata', 'canonical 链接',     'meta_attr',11),
    ('copyright',     '版权声明', 'string',   'metadata', '版权声明文本',       'css',      12),
    ('category',     '分类',     'string', 'classification', '文章分类',         'css',      13),
    ('tags',         '标签',     'string', 'classification', '标签列表',         'css',      14),
    ('content_type', '内容类型', 'string', 'classification', '如视频/文章/资源', 'meta_attr',15),
    ('language',     '语言',     'string', 'classification', '内容语言',         'meta_attr',16),
    ('region',       '地区',     'string', 'classification', '内容地区',         'meta_attr',17),
    ('view_count',    '浏览量', 'number', 'interaction', '浏览次数', 'regex', 18),
    ('comment_count', '评论数', 'number', 'interaction', '评论条数', 'regex', 19),
    ('like_count',    '点赞数', 'number', 'interaction', '点赞次数', 'regex', 20),
    ('rating',        '评分',   'number', 'interaction', '评分（5 分制）', 'regex', 21),
    ('file_size', '附件大小', 'string', 'resource', '附件文件大小',     'regex', 22),
    ('duration',  '时长',     'string', 'resource', '音视频时长',       'regex', 23),
    ('version',   '版本号',   'string', 'resource', '软件/资源版本号',  'regex', 24)
ON CONFLICT (key) DO NOTHING;
