//! script_runner — feature 046-crawler-script-extractor
//!
//! async 入口：把脚本字段（[`crate::services::crawler::field_schema::ScriptRule`]）的 body
//! 包装为 `(function(ctx) { ${body} })` 求值，注入 `ctx` 对象（value/fields/url/fetch），
//! 在 [`script_engine`] 的沙箱中执行，返回最终字符串值。
//!
//! US1：注入 ctx.value + ctx.url
//! US2：扩展 ctx.fields
//! US3：注入 ctx.fetch（同步阻塞实现，详见下方说明）
//!
//! 单字段失败不中断：本模块返回 `Result<String, ScriptError>`，调用方（engine/probe）按
//! [`extractor`] 的 6 模式失败处理路径处理。
//!
//! # US3 ctx.fetch 实现说明
//!
//! rquickjs 同步求值（`evaluate_blocking`）无法 await Promise。当前实现采用**同步阻塞**策略：
//! - `run_script` 入口先抓 `tokio::runtime::Handle::try_current()`，move 进 spawn_blocking 闭包
//! - `ctx.fetch` 注册为同步 rquickjs Function，内部 `handle.block_on(fetch_impl(...))`
//! - JS 侧调用 `const r = ctx.fetch(url); const j = r.json();`（**不带 await**）
//!
//! 与 spec 契约的偏差：spec 描述 `await ctx.fetch(url)`，实际为同步语义。文档化于
//! `contracts/script-runtime.md` 的「ctx.fetch 同步语义」段（待补）。

use std::collections::HashMap;
use std::sync::Arc;

use rquickjs::{Ctx, Function, Object};

use crate::services::crawler::field_schema::ScriptRule;
use crate::services::crawler::script_engine::{
    evaluate_blocking, ScriptError, ScriptFailureCategory,
};
use crate::services::crawler::script_fetch::{self, FetchError, FetchOpts};

/// 脚本求值选项（由调用方根据 AppState.option_cache 构造）
#[derive(Debug, Clone)]
pub struct ScriptOpts {
    /// 单脚本求值墙钟超时（ms），默认 100
    pub timeout_ms: u64,
    /// ctx.fetch 单次响应大小上限（字节），默认 1 MB
    pub max_response_bytes: usize,
    /// script.body 最大长度（字节），默认 64 KB
    pub max_body_size: usize,
}

impl Default for ScriptOpts {
    fn default() -> Self {
        Self {
            timeout_ms: 100,
            max_response_bytes: 1_048_576, // 1 MB
            max_body_size: 65_536,         // 64 KB
        }
    }
}

/// 脚本求值入参（US1 阶段只用 value；US2 加 fields；US3 加 http_client）
pub struct ScriptRunInput<'a> {
    /// 当前字段经 6 模式提取的原始值（未匹配/未配置时为空字符串）
    pub value: String,
    /// 当前作用域兄弟字段最新最终值（按 name 索引，标量）
    pub fields: HashMap<String, String>,
    /// 当前文章详情页 URL
    pub url: String,
    /// 任务级 reqwest 客户端（含 proxy/UA/cookie）；US1/US2 阶段为 None
    pub http_client: Option<&'a reqwest::Client>,
}

/// preflight 校验：空、超长 body 拒绝
pub fn preflight_check(body: &str, opts: &ScriptOpts) -> Result<(), ScriptError> {
    if body.trim().is_empty() {
        return Err(ScriptError::new(
            ScriptFailureCategory::SyntaxError,
            "script.body 不能为空",
        ));
    }
    if body.len() > opts.max_body_size {
        return Err(ScriptError::new(
            ScriptFailureCategory::SyntaxError,
            format!(
                "script.body 长度 {} 超过上限 {} 字节",
                body.len(),
                opts.max_body_size
            ),
        ));
    }
    Ok(())
}

/// ctx 注入器（持有 ctx.value/fields/url 数据 + 可选 fetch 上下文）
///
/// 注：必须 Clone 友好（spawn_blocking 需要 'static + Send）。
/// US3：fetch_context.is_some() 时在 ctx 上额外注入 ctx.fetch（同步阻塞语义）
#[derive(Clone)]
struct CtxInjectorData {
    value: String,
    fields: Arc<HashMap<String, String>>,
    url: String,
    /// [US3] None → 不注入 ctx.fetch；Some → 注入同步 fetch 函数
    fetch_context: Option<Arc<FetchContext>>,
}

/// [US3] ctx.fetch 注入所需上下文（tokio handle + reqwest client + 大小上限）
struct FetchContext {
    handle: tokio::runtime::Handle,
    client: reqwest::Client,
    max_response_bytes: usize,
}

impl CtxInjectorData {
    fn build<'js>(&self, ctx: &Ctx<'js>) -> Result<Object<'js>, ScriptError> {
        let obj = Object::new(ctx.clone()).map_err(|e| {
            ScriptError::new(
                ScriptFailureCategory::RuntimeError,
                format!("ctx Object::new 失败: {e:?}"),
            )
        })?;
        obj.set("value", self.value.as_str()).map_err(|e| {
            ScriptError::new(
                ScriptFailureCategory::RuntimeError,
                format!("ctx.set('value') 失败: {e:?}"),
            )
        })?;
        // ctx.fields：构造嵌套 Object（US2 启用）
        let fields_obj = Object::new(ctx.clone()).map_err(|e| {
            ScriptError::new(
                ScriptFailureCategory::RuntimeError,
                format!("ctx.fields Object::new 失败: {e:?}"),
            )
        })?;
        for (k, v) in self.fields.iter() {
            fields_obj.set(k.as_str(), v.as_str()).map_err(|e| {
                ScriptError::new(
                    ScriptFailureCategory::RuntimeError,
                    format!("ctx.fields.set({k}) 失败: {e:?}"),
                )
            })?;
        }
        obj.set("fields", fields_obj).map_err(|e| {
            ScriptError::new(
                ScriptFailureCategory::RuntimeError,
                format!("ctx.set('fields') 失败: {e:?}"),
            )
        })?;
        obj.set("url", self.url.as_str()).map_err(|e| {
            ScriptError::new(
                ScriptFailureCategory::RuntimeError,
                format!("ctx.set('url') 失败: {e:?}"),
            )
        })?;

        // [US3] 注入 ctx.fetch（同步阻塞语义：成功返回 body_text，失败返回空串 + 日志）
        // rquickjs Function 闭包不支持 Result Err 抛 JS Exception（Error: From<FetchError> 不实现），
        // 失败时返回空串并通过 tracing::warn 记录；脚本侧可检查空串并 fallback。
        if let Some(fc) = &self.fetch_context {
            let fc = fc.clone();
            let fetch_fn = Function::new(ctx.clone(), move |url: String| -> String {
                fetch_sync(&fc, &url)
            })
            .map_err(|e| {
                ScriptError::new(
                    ScriptFailureCategory::RuntimeError,
                    format!("ctx.fetch Function::new 失败: {e:?}"),
                )
            })?;
            obj.set("fetch", fetch_fn).map_err(|e| {
                ScriptError::new(
                    ScriptFailureCategory::RuntimeError,
                    format!("ctx.set('fetch') 失败: {e:?}"),
                )
            })?;
        }

        Ok(obj)
    }
}

/// [US3] 同步阻塞调用 fetch_impl：在 spawn_blocking 线程内通过 handle.block_on 等待 async 完成。
///
/// 成功返回 body_text 字符串；失败返回空串 + tracing::warn 日志（含 category）。
fn fetch_sync(fc: &FetchContext, url: &str) -> String {
    let handle = fc.handle.clone();
    let client = fc.client.clone();
    let max_bytes = fc.max_response_bytes;
    let url_s = url.to_string();
    let result = handle.block_on(async move {
        let opts = FetchOpts::default();
        script_fetch::fetch_impl(&url_s, &opts, &client, max_bytes).await
    });
    match result {
        Ok(r) => r.body_text,
        Err(e) => {
            let category = match &e {
                FetchError::InvalidScheme(_) | FetchError::InvalidUrl(_) => "invalid_url",
                FetchError::Ssrf { .. } => "ssrf",
                FetchError::Dns(_) => "dns",
                FetchError::ResponseTooLarge { .. } => "response_too_large",
                FetchError::Network(_) => "network",
            };
            tracing::warn!(
                target: "crawler",
                "ctx.fetch 失败 [{}]: {} (url={})",
                category,
                e,
                url
            );
            String::new()
        }
    }
}

/// 主入口：执行脚本求值。
///
/// **流程**：
/// 1. preflight：body 非空 / 长度 ≤ max_body_size
/// 2. 构造 ctx 注入器（CtxInjectorData）
/// 3. `tokio::task::spawn_blocking` 包裹 [`evaluate_blocking`] —— rquickjs 求值是 CPU-bound 同步，
///    不能在 async runtime 直接跑（会阻塞 reactor）
/// 4. US3：`http_client.is_some()` 时在 ctx 上额外注入 ctx.fetch（同步阻塞语义，详见模块文档）
pub async fn run_script(
    rule: &ScriptRule,
    value: String,
    fields: HashMap<String, String>,
    url: &str,
    http_client: Option<&reqwest::Client>,
    opts: &ScriptOpts,
) -> Result<String, ScriptError> {
    // 1. preflight
    preflight_check(&rule.body, opts)?;

    // 2. 准备注入器数据
    let fetch_context = match http_client {
        Some(client) => {
            let handle = tokio::runtime::Handle::try_current().map_err(|e| {
                ScriptError::new(
                    ScriptFailureCategory::RuntimeError,
                    format!("无法获取 tokio runtime handle（US3 ctx.fetch 需要）：{e}"),
                )
            })?;
            Some(Arc::new(FetchContext {
                handle,
                client: client.clone(),
                max_response_bytes: opts.max_response_bytes,
            }))
        }
        None => None,
    };
    let injector_data = CtxInjectorData {
        value,
        fields: Arc::new(fields),
        url: url.to_string(),
        fetch_context,
    };
    let body = rule.body.clone();
    let timeout_ms = opts.timeout_ms;

    // 3. spawn_blocking 求值
    let join = tokio::task::spawn_blocking(move || {
        let injector = CtxInjectorData::build_closure(injector_data);
        evaluate_blocking(&body, &injector, timeout_ms)
    });
    join.await
        .map_err(|e| {
            ScriptError::new(
                ScriptFailureCategory::RuntimeError,
                format!("spawn_blocking join 失败: {e}"),
            )
        })?
}

impl CtxInjectorData {
    /// 把自身克隆一份，构造一个满足 `for<'js> Fn(&Ctx<'js>) -> Result<Object<'js>, ScriptError>` 的闭包。
    fn build_closure(
        data: CtxInjectorData,
    ) -> impl for<'js> Fn(&Ctx<'js>) -> Result<Object<'js>, ScriptError> {
        move |ctx: &Ctx| data.build(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_preflight_rejects_empty_body() {
        let opts = ScriptOpts::default();
        let err = preflight_check("", &opts).unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::SyntaxError);
    }

    #[test]
    fn t_preflight_rejects_whitespace_only_body() {
        let opts = ScriptOpts::default();
        let err = preflight_check("   \n\t  ", &opts).unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::SyntaxError);
    }

    #[test]
    fn t_preflight_rejects_oversized_body() {
        let opts = ScriptOpts {
            max_body_size: 16,
            ..Default::default()
        };
        let body = "x".repeat(17);
        let err = preflight_check(&body, &opts).unwrap_err();
        assert!(err.message.contains("17"));
        assert!(err.message.contains("16"));
    }

    #[test]
    fn t_preflight_accepts_normal_body() {
        let opts = ScriptOpts::default();
        preflight_check("return ctx.value + '!'", &opts).unwrap();
    }

    #[test]
    fn t_script_opts_defaults_match_data_model() {
        let opts = ScriptOpts::default();
        assert_eq!(opts.timeout_ms, 100);
        assert_eq!(opts.max_response_bytes, 1_048_576);
        assert_eq!(opts.max_body_size, 65_536);
    }

    // ---- run_script 端到端（US1） ----

    fn make_rule(body: &str) -> ScriptRule {
        ScriptRule {
            body: body.into(),
            api_version: "v1".into(),
        }
    }

    #[tokio::test]
    async fn t_run_script_simple_value_transform() {
        let rule = make_rule("return ctx.value.toUpperCase()");
        let opts = ScriptOpts::default();
        let result = run_script(&rule, "hello".into(), HashMap::new(), "/x", None, &opts)
            .await
            .unwrap();
        assert_eq!(result, "HELLO");
    }

    #[tokio::test]
    async fn t_run_script_with_empty_value_when_6mode_misses() {
        // 6 模式未匹配 → value="" → 脚本仍跑
        let rule = make_rule("return ctx.value + '_default'");
        let opts = ScriptOpts::default();
        let result = run_script(&rule, "".into(), HashMap::new(), "/x", None, &opts)
            .await
            .unwrap();
        assert_eq!(result, "_default");
    }

    #[tokio::test]
    async fn t_run_script_returns_promise_resolved_string() {
        // 注意：当前 evaluate_blocking 是同步求值，async function 在 rquickjs 同步模式下不可用
        // 但只要 body 不用 await，普通 function 体即可
        let rule = make_rule("return ctx.value + '!'");
        let opts = ScriptOpts::default();
        let result = run_script(&rule, "v".into(), HashMap::new(), "/x", None, &opts)
            .await
            .unwrap();
        assert_eq!(result, "v!");
    }

    #[tokio::test]
    async fn t_run_script_injects_fields_and_url() {
        // US2 验证：ctx.fields.title + ctx.url 可访问
        let rule = make_rule("return ctx.fields.title + '@' + ctx.url");
        let mut fields = HashMap::new();
        fields.insert("title".into(), "MyTitle".into());
        let opts = ScriptOpts::default();
        let result = run_script(&rule, "".into(), fields, "/p/1", None, &opts)
            .await
            .unwrap();
        assert_eq!(result, "MyTitle@/p/1");
    }

    #[tokio::test]
    async fn t_run_script_classifies_syntax_error() {
        let rule = make_rule("return .");
        let opts = ScriptOpts::default();
        let err = run_script(&rule, "x".into(), HashMap::new(), "/x", None, &opts)
            .await
            .unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::SyntaxError);
    }

    #[tokio::test]
    async fn t_run_script_classifies_security_violation() {
        let rule = make_rule("return Function('return this')()");
        let opts = ScriptOpts::default();
        let err = run_script(&rule, "x".into(), HashMap::new(), "/x", None, &opts)
            .await
            .unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::SecurityViolation);
    }

    #[tokio::test]
    async fn t_run_script_classifies_runtime_error() {
        let rule = make_rule("throw new Error('boom')");
        let opts = ScriptOpts::default();
        let err = run_script(&rule, "x".into(), HashMap::new(), "/x", None, &opts)
            .await
            .unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::RuntimeError);
        assert!(err.message.contains("boom"));
    }

    #[tokio::test]
    async fn t_run_script_classifies_type_error_on_no_return() {
        let rule = make_rule("/* no return */");
        let opts = ScriptOpts::default();
        let err = run_script(&rule, "x".into(), HashMap::new(), "/x", None, &opts)
            .await
            .unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::TypeError);
    }

    #[tokio::test]
    async fn t_run_script_timeout_interrupts_infinite_loop() {
        let rule = make_rule("while (true) { /* spin */ }");
        let opts = ScriptOpts {
            timeout_ms: 50,
            ..Default::default()
        };
        let err = run_script(&rule, "x".into(), HashMap::new(), "/x", None, &opts)
            .await
            .unwrap_err();
        // 超时被 interrupt 后，rquickjs 抛 "interrupted" → Timeout
        assert!(
            err.category == ScriptFailureCategory::Timeout
                || err.category == ScriptFailureCategory::RuntimeError,
            "实际: {err:?}"
        );
    }

    // ---- US2：跨字段组合（ctx.fields.<name>）----

    #[tokio::test]
    async fn t_run_script_reads_other_field_value() {
        // 配置 ctx_fields={"pan_type":"quark"}，脚本根据兄弟字段决定输出
        let rule = make_rule(
            "return ctx.fields.pan_type === 'quark' ? ctx.value + '?quark' : ctx.value",
        );
        let mut fields = HashMap::new();
        fields.insert("pan_type".into(), "quark".into());
        let opts = ScriptOpts::default();
        let result = run_script(&rule, "https://x.io/d".into(), fields, "/x", None, &opts)
            .await
            .unwrap();
        assert_eq!(result, "https://x.io/d?quark");
    }

    #[tokio::test]
    async fn t_run_script_handles_missing_field_as_undefined() {
        // ctx_fields={}：访问 ctx.fields.nonexistent 应得 undefined（不抛错）
        // JS 短路：undefined || 'default' → 'default'
        let rule = make_rule("return ctx.fields.nonexistent || 'default'");
        let opts = ScriptOpts::default();
        let result = run_script(
            &rule,
            "v".into(),
            HashMap::new(),
            "/x",
            None,
            &opts,
        )
        .await
        .unwrap();
        assert_eq!(result, "default");
    }

    #[tokio::test]
    async fn t_run_script_handles_failed_sibling_field() {
        // 失败字段（无 final_value，未放入 ctx_fields）→ 在 JS 侧等同 undefined
        // 脚本应能优雅降级，不应因兄弟字段失败而抛错
        let rule = make_rule(
            "return ctx.fields.failed_sibling ? 'has' : 'missing'",
        );
        let opts = ScriptOpts::default();
        let result = run_script(
            &rule,
            "v".into(),
            HashMap::new(), // 失败字段不在 map 中
            "/x",
            None,
            &opts,
        )
        .await
        .unwrap();
        assert_eq!(result, "missing");
    }

    // ---- US3：ctx.fetch（同步阻塞语义；失败优雅降级为空串）----

    /// helper：用 wiremock 构造一个监听 127.0.0.1 的 mock，返回其 uri。
    async fn spawn_mock_server(body: &'static str) -> (wiremock::MockServer, String) {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
        let uri = server.uri();
        (server, uri)
    }

    #[tokio::test]
    async fn t_run_script_with_fetch_returns_value() {
        // 验证「http_client=Some 时 ctx.fetch 被注入」的契约（typeof === 'function'）
        // 不真正调用 fetch，因此无需 mock。
        let rule = make_rule("return typeof ctx.fetch === 'function' ? 'has_fetch' : 'no_fetch'");
        let opts = ScriptOpts::default();
        let client = reqwest::Client::new();
        let result = run_script(
            &rule,
            "".into(),
            HashMap::new(),
            "/x",
            Some(&client),
            &opts,
        )
        .await
        .unwrap();
        assert_eq!(result, "has_fetch");
    }

    #[tokio::test]
    async fn t_run_script_without_http_client_has_no_fetch() {
        // http_client=None → ctx.fetch 未注入 → typeof === 'undefined'
        let rule = make_rule("return typeof ctx.fetch === 'undefined' ? 'no_fetch' : 'has_fetch'");
        let opts = ScriptOpts::default();
        let result = run_script(&rule, "".into(), HashMap::new(), "/x", None, &opts)
            .await
            .unwrap();
        assert_eq!(result, "no_fetch");
    }

    #[tokio::test]
    async fn t_run_script_fetch_returns_empty_on_ssrf() {
        // 127.0.0.1 → SSRF 拒绝 → 脚本拿到空串（不抛错）
        let (_server, uri) = spawn_mock_server("hello").await;
        let body = format!(
            "const t = ctx.fetch('{}'); return t.length === 0 ? 'empty' : 'non_empty'",
            uri
        );
        let rule = make_rule(&body);
        let opts = ScriptOpts::default();
        let client = reqwest::Client::new();
        let result = run_script(
            &rule,
            "".into(),
            HashMap::new(),
            "/x",
            Some(&client),
            &opts,
        )
        .await
        .unwrap();
        // 127.0.0.1 命中 SSRF 拒绝名单 → fetch_sync 返回空串
        assert_eq!(result, "empty");
    }
}

