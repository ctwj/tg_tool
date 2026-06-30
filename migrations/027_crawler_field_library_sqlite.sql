-- 027: 预置字段库表 crawler_field_library (SQLite) — feature 043-crawler-configurator
-- data-model.md E2：≥ 20 类预置字段供字段配置器渲染"添加字段"勾选清单。
-- 种子数据 INSERT OR IGNORE 保证幂等（应用层启动期会再次检查空表并补种）。

CREATE TABLE IF NOT EXISTS crawler_field_library (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    key TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    field_type TEXT NOT NULL,
    category TEXT NOT NULL,
    description TEXT,
    suggested_extractor TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_field_library_category ON crawler_field_library(category, sort_order);

-- 种子数据：5 个分类共 26 条预置字段（data-model.md E2 + 资源下载场景扩展）
-- 基础字段
INSERT OR IGNORE INTO crawler_field_library (key, display_name, field_type, category, description, suggested_extractor, sort_order) VALUES
    ('title',        '标题',     'string', 'basic', '文章标题',                   'css',     1),
    ('url',          '链接',     'url',    'basic', '详情页链接',                 'css',     2),
    ('cover',        '封面',     'image',  'basic', '列表卡片封面图',             'css',     3),
    ('thumbnail',    '缩略图',   'image',  'basic', '小尺寸预览图',               'css',     4),
    ('description',  '描述',     'text',   'basic', '摘要/副标题/简介',           'css',     5),
    ('content',      '正文',     'text',   'basic', '详情页正文 HTML',            'css',     6);

-- 元数据
INSERT OR IGNORE INTO crawler_field_library (key, display_name, field_type, category, description, suggested_extractor, sort_order) VALUES
    ('author',        '作者',     'string',   'metadata', '文章作者',           'css',      1),
    ('published_at',  '发布时间', 'datetime', 'metadata', '发布时间',           'css',      2),
    ('updated_at',    '更新时间', 'datetime', 'metadata', '最后更新时间',       'css',      3),
    ('source_site',   '来源站点', 'string',   'metadata', '来源站点名',         'meta_attr',4),
    ('canonical_url', '原文链接', 'url',      'metadata', 'canonical 链接',     'meta_attr',5),
    ('copyright',     '版权声明', 'string',   'metadata', '版权声明文本',       'css',      6);

-- 分类与标签
INSERT OR IGNORE INTO crawler_field_library (key, display_name, field_type, category, description, suggested_extractor, sort_order) VALUES
    ('category',     '分类',     'string', 'classification', '文章分类',         'css',      1),
    ('tags',         '标签',     'string', 'classification', '标签列表',         'css',      2),
    ('content_type', '内容类型', 'string', 'classification', '如视频/文章/资源', 'meta_attr',3),
    ('language',     '语言',     'string', 'classification', '内容语言',         'meta_attr',4),
    ('region',       '地区',     'string', 'classification', '内容地区',         'meta_attr',5);

-- 互动指标
INSERT OR IGNORE INTO crawler_field_library (key, display_name, field_type, category, description, suggested_extractor, sort_order) VALUES
    ('view_count',    '浏览量', 'number', 'interaction', '浏览次数', 'regex', 1),
    ('comment_count', '评论数', 'number', 'interaction', '评论条数', 'regex', 2),
    ('like_count',    '点赞数', 'number', 'interaction', '点赞次数', 'regex', 3),
    ('rating',        '评分',   'number', 'interaction', '评分（5 分制）', 'regex', 4);

-- 资源属性（download_url / resource_name 是资源类核心，sort_order 排最前）
INSERT OR IGNORE INTO crawler_field_library (key, display_name, field_type, category, description, suggested_extractor, sort_order) VALUES
    ('download_url',  '下载地址', 'url',    'resource', '资源下载地址（直链或网盘）。可能需用 follow_url 模式两阶段提取',                    'css',   1),
    ('resource_name', '资源名',   'string', 'resource', '资源名称（区别于文章标题 title，适用于一篇文章列多个资源的场景）',                  'css',   2),
    ('file_size',     '附件大小', 'string', 'resource', '附件文件大小',                                                                  'regex', 3),
    ('duration',      '时长',     'string', 'resource', '音视频时长',                                                                    'regex', 4),
    ('version',       '版本号',   'string', 'resource', '软件/资源版本号',                                                               'regex', 5);
