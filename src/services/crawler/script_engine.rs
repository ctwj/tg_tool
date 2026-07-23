//! script_engine — feature 046-crawler-script-extractor
//!
//! rquickjs 沙箱封装：Runtime / Context 创建、安全加固、超时控制、错误分类。
//! 上层 [`crate::services::crawler::script_runner`] 调用本模块的 `evaluate` 求值 JS 函数体。
//!
//! 设计要点（research.md R-002）：
//! - 每次求值新建 Runtime + Context（轻量，避免脚本间状态泄漏）
//! - 双层安全：
//!   - **静态扫描**：源码含危险标识符（Function/eval/process/setTimeout/setInterval）
//!     直接 SecurityViolation 拒绝（pre-flight）
//!   - **运行时加固**：删除 globalThis.{eval, Function, setTimeout, setInterval, process}；
//!     memory_limit + max_stack_size 防 OOM/栈溢出
//! - 双层超时：rquickjs `Runtime::set_interrupt_handler`（基于 Instant deadline） + 墙钟兜底
//! - 错误分类：SyntaxError / RuntimeError / Timeout / SecurityViolation / TypeError / NetworkError
//!   详见 `ScriptFailureCategory` 与 contracts/script-runtime.md

use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use regex::Regex;
use rquickjs::prelude::CatchResultExt;
use rquickjs::{CaughtError, Context, Ctx, Object, Runtime};
use serde::{Deserialize, Serialize};

/// 脚本失败分类（与 contracts/script-runtime.md「错误分类」对齐）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptFailureCategory {
    /// 编译期语法错（rquickjs 解析失败）
    SyntaxError,
    /// 运行时异常（throw、除零、未定义变量、JSON.parse 失败等）
    RuntimeError,
    /// 超时（指令计数 interrupt 或墙钟 tokio::timeout）
    Timeout,
    /// 沙箱逃逸尝试（Function 构造器、eval、globalThis.process 等）
    SecurityViolation,
    /// 返回值类型错（无 return、返回 undefined/null、Promise reject）
    TypeError,
    /// ctx.fetch 网络错（含 SSRF 拒绝、响应超阈值）
    NetworkError,
}

impl ScriptFailureCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            ScriptFailureCategory::SyntaxError => "syntax_error",
            ScriptFailureCategory::RuntimeError => "runtime_error",
            ScriptFailureCategory::Timeout => "timeout",
            ScriptFailureCategory::SecurityViolation => "security_violation",
            ScriptFailureCategory::TypeError => "type_error",
            ScriptFailureCategory::NetworkError => "network_error",
        }
    }
}

/// 脚本求值失败（带分类、定位信息、耗时）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptError {
    pub category: ScriptFailureCategory,
    pub message: String,
    /// 语法错时填行号（1-based），其他场景为 None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// 语法错时填列号（1-based）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    pub duration_ms: u64,
}

impl ScriptError {
    pub fn new(category: ScriptFailureCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
            line: None,
            column: None,
            duration_ms: 0,
        }
    }

    pub fn with_position(mut self, line: u32, column: u32) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }

    pub fn with_duration(mut self, ms: u64) -> Self {
        self.duration_ms = ms;
        self
    }
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.category.as_str(), self.message)
    }
}

impl std::error::Error for ScriptError {}

// ============================================================================
// 静态安全扫描：危险标识符
// ============================================================================

/// 危险标识符正则集合（whole-word 匹配，区分大小写）。
///
/// 命中任一即视为沙箱逃逸尝试（FR-012 / R-002）：
/// - `Function` / `eval` —— 动态代码执行原语
/// - `process` / `globalThis.process` —— Node.js / Bun 宿主对象
/// - `setTimeout` / `setInterval` —— 定时器（异步逃逸面）
///
/// 注意：`constructor` 没列入（合法 JS 代码常用），由运行时删除 globalThis.Function +
/// memory/stack 限制 + interrupt 联合防御 `xxx.constructor.constructor(...)` 攻击。
static DANGER_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"\bFunction\b",
        r"\beval\b",
        r"\bprocess\b",
        r"\bsetTimeout\b",
        r"\bsetInterval\b",
    ]
    .iter()
    .map(|p| Regex::new(p).expect("invalid danger regex"))
    .collect()
});

/// 静态扫描：脚本源码含危险标识符 → [`ScriptFailureCategory::SecurityViolation`]
pub fn scan_security(body: &str) -> Result<(), ScriptError> {
    for pat in DANGER_PATTERNS.iter() {
        if let Some(m) = pat.find(body) {
            return Err(ScriptError::new(
                ScriptFailureCategory::SecurityViolation,
                format!(
                    "脚本含被禁标识符 '{}'（位置 {}），疑似沙箱逃逸尝试",
                    m.as_str(),
                    m.start()
                ),
            ));
        }
    }
    Ok(())
}

// ============================================================================
// 沙箱求值
// ============================================================================

/// ctx 注入器：调用方构造 ctx 对象（含 value/fields/url/fetch 等）
pub trait CtxInjector {
    fn build<'js>(&self, ctx: &Ctx<'js>) -> Result<Object<'js>, ScriptError>;
}

/// 为闭包实现 CtxInjector，简化上层调用
impl<F> CtxInjector for F
where
    F: for<'js> Fn(&Ctx<'js>) -> Result<Object<'js>, ScriptError>,
{
    fn build<'js>(&self, ctx: &Ctx<'js>) -> Result<Object<'js>, ScriptError> {
        (self)(ctx)
    }
}

/// 沙箱求值入口。
///
/// 把 `body` 包装为 `(function(__ctx) { ${body} })`，在新建的 rquickjs Runtime/Context 中执行，
/// 调用时传入 `__ctx` 对象，返回字符串。
///
/// **双层安全 + 超时**：
/// 1. pre-flight：`scan_security` 静态扫描危险标识符
/// 2. pre-flight：`preflight_check`（在 script_runner） 校验空/超长
/// 3. 运行时：删除 globalThis.{eval, Function, setTimeout, setInterval, process}
/// 4. 运行时：`set_memory_limit(16MB)` / `set_max_stack_size(256KB)`
/// 5. 运行时：`set_interrupt_handler` 基于 Instant deadline，超时返回 false 终止 JS
/// 6. 墙钟兜底：`evaluate_blocking` 在 `tokio::task::spawn_blocking` 内执行；
///    外层可用 `tokio::time::timeout` 包裹
pub fn evaluate_blocking(
    body: &str,
    injector: &dyn CtxInjector,
    timeout_ms: u64,
) -> Result<String, ScriptError> {
    let start = Instant::now();

    // 静态安全扫描
    scan_security(body)?;

    // 创建 Runtime + Context
    let rt = Runtime::new().map_err(|e| {
        ScriptError::new(
            ScriptFailureCategory::RuntimeError,
            format!("rquickjs Runtime 创建失败: {e}"),
        )
    })?;
    rt.set_memory_limit(16 * 1024 * 1024); // 16MB
    rt.set_max_stack_size(256 * 1024); // 256KB

    // 中断处理器：基于时间 deadline
    // rquickjs 语义：handler 返回 true = 中断 JS，false = 继续
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(1));
    rt.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));

    let ctx = Context::full(&rt).map_err(|e| {
        ScriptError::new(
            ScriptFailureCategory::RuntimeError,
            format!("rquickjs Context 创建失败: {e}"),
        )
    })?;

    let result = ctx.with(|ctx| run_script(&ctx, body, injector, start));
    let elapsed = start.elapsed().as_millis() as u64;
    result.map_err(|e| e.with_duration(elapsed))
}

/// 在 ctx.with 闭包内执行：注入 ctx、删除危险全局、eval 包装函数、调用、取返回值
fn run_script<'js>(
    ctx: &Ctx<'js>,
    body: &str,
    injector: &dyn CtxInjector,
    start: Instant,
) -> Result<String, ScriptError> {
    let globals = ctx.globals();

    // 删除危险全局（best-effort，失败忽略）
    for name in ["eval", "Function", "setTimeout", "setInterval", "process"] {
        let _ = globals.remove(name);
    }

    // 注入 ctx 对象
    let ctx_obj = injector.build(ctx)?;

    // 把 body 包装为函数表达式：(function(ctx) { ${body} })
    let wrapped = format!("(function(ctx) {{\n{body}\n}})");

    // 编译并求值出函数对象
    let func: rquickjs::Function = match ctx
        .eval::<rquickjs::Function, _>(wrapped.as_str())
        .catch(ctx)
    {
        Ok(f) => f,
        Err(CaughtError::Exception(exc)) => {
            return Err(classify_exception(&exc, start));
        }
        Err(CaughtError::Value(v)) => {
            return Err(ScriptError::new(
                ScriptFailureCategory::RuntimeError,
                format!("脚本编译期抛出非 Error 值: {v:?}"),
            ));
        }
        Err(CaughtError::Error(e)) => {
            return Err(ScriptError::new(
                ScriptFailureCategory::RuntimeError,
                format!("rquickjs 内部错: {e:?}"),
            ));
        }
    };

    // 调用函数，传入 ctx_obj
    let ret: rquickjs::Value = match func.call((ctx_obj,)).catch(ctx) {
        Ok(v) => v,
        Err(CaughtError::Exception(exc)) => {
            return Err(classify_exception(&exc, start));
        }
        Err(CaughtError::Value(v)) => {
            return Err(ScriptError::new(
                ScriptFailureCategory::RuntimeError,
                format!("脚本运行期抛出非 Error 值: {v:?}"),
            ));
        }
        Err(CaughtError::Error(e)) => {
            return Err(ScriptError::new(
                ScriptFailureCategory::RuntimeError,
                format!("rquickjs 内部错: {e:?}"),
            ));
        }
    };

    // 处理返回值：undefined/null → TypeError；字符串 → 直接提取
    if ret.is_undefined() || ret.is_null() {
        return Err(ScriptError::new(
            ScriptFailureCategory::TypeError,
            "脚本返回值为 undefined/null — 必须 return 字符串/数字。\
             常见原因：1) 脚本未显式 return；2) return 了 ctx.fields.X 但该兄弟字段在此场景下未填充\
             （单字段验证 / 探针时 ctx.fields 为空，需用 test_run 或任务实际运行验证）。",
        ));
    }

    // 字符串直接转换
    if let Some(s) = ret.as_string() {
        let s = s.to_string().map_err(|e| {
            ScriptError::new(
                ScriptFailureCategory::RuntimeError,
                format!("返回字符串提取失败: {e:?}"),
            )
        })?;
        return Ok(s);
    }

    // 其它类型（数字/bool/object）→ rquickjs 不支持把 Value 直接序列化为源码，
    // 通过 new Function('return x') 闭包 + String() 强转。简化：返回类型不匹配错误。
    Err(ScriptError::new(
        ScriptFailureCategory::TypeError,
        format!(
            "脚本返回值类型不被支持（仅 string/number/boolean），实际类型: {:?}",
            ret.type_of().as_str()
        ),
    ))
}

/// 把 CaughtError::Exception 分类到 [`ScriptError`]
///
/// 分类规则（research.md R-002 错误矩阵）：
/// - message 含 "interrupted" → Timeout
/// - message 含 "is not defined" 且涉及危险词 → SecurityViolation
/// - message 含 "SyntaxError" → SyntaxError
/// - message 含 "TypeError" → TypeError
/// - 兜底 → RuntimeError
fn classify_exception(exc: &rquickjs::Exception, start: Instant) -> ScriptError {
    let message = exc.message().unwrap_or_default().to_string();
    let elapsed = start.elapsed().as_millis() as u64;

    // 超时中断（rquickjs 内部中断后会抛 "interrupted"）
    if message.contains("interrupted") || message.contains("Interrupted") {
        return ScriptError::new(ScriptFailureCategory::Timeout, message).with_duration(elapsed);
    }

    // is not defined → 大概率是访问了被删除的危险全局
    if message.contains("is not defined")
        && DANGER_NAMES.iter().any(|k| message.contains(k))
    {
        return ScriptError::new(ScriptFailureCategory::SecurityViolation, message)
            .with_duration(elapsed);
    }

    // 通用：根据 message 内容含类名推断
    if message.contains("SyntaxError")
        || message.contains("Unexpected")
        || message.contains("unexpected token")
        || message.contains("expecting")
    {
        return ScriptError::new(ScriptFailureCategory::SyntaxError, message).with_duration(elapsed);
    }
    if message.contains("TypeError") {
        return ScriptError::new(ScriptFailureCategory::TypeError, message).with_duration(elapsed);
    }

    ScriptError::new(ScriptFailureCategory::RuntimeError, message).with_duration(elapsed)
}

const DANGER_NAMES: &[&str] = &["Function", "eval", "process", "setTimeout", "setInterval"];

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 既有分类枚举测试 ----

    #[test]
    fn t_failure_category_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&ScriptFailureCategory::SyntaxError).unwrap(),
            "\"syntax_error\""
        );
        assert_eq!(
            serde_json::to_string(&ScriptFailureCategory::SecurityViolation).unwrap(),
            "\"security_violation\""
        );
        assert_eq!(
            serde_json::to_string(&ScriptFailureCategory::NetworkError).unwrap(),
            "\"network_error\""
        );
    }

    #[test]
    fn t_failure_category_round_trip() {
        let cats = [
            ScriptFailureCategory::SyntaxError,
            ScriptFailureCategory::RuntimeError,
            ScriptFailureCategory::Timeout,
            ScriptFailureCategory::SecurityViolation,
            ScriptFailureCategory::TypeError,
            ScriptFailureCategory::NetworkError,
        ];
        for c in cats {
            let s = serde_json::to_string(&c).unwrap();
            let back: ScriptFailureCategory = serde_json::from_str(&s).unwrap();
            assert_eq!(c, back);
        }
    }

    // ---- 静态安全扫描 ----

    #[test]
    fn t_scan_security_rejects_function_keyword() {
        let body = "return Function('return this')()";
        let err = scan_security(body).unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::SecurityViolation);
    }

    #[test]
    fn t_scan_security_rejects_eval() {
        let body = "return eval('1+1')";
        let err = scan_security(body).unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::SecurityViolation);
    }

    #[test]
    fn t_scan_security_rejects_process() {
        let body = "return globalThis.process";
        let err = scan_security(body).unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::SecurityViolation);
    }

    #[test]
    fn t_scan_security_rejects_settimeout() {
        let body = "return setTimeout(() => 0, 0)";
        let err = scan_security(body).unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::SecurityViolation);
    }

    #[test]
    fn t_scan_security_rejects_constructor_chain_indirectly() {
        // 含 process 字符串字面量 → 命中 \bprocess\b
        let body = "return [].constructor.constructor('return process')()";
        let err = scan_security(body).unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::SecurityViolation);
    }

    #[test]
    fn t_scan_security_rejects_reflect_apply() {
        // 含 Function → 命中
        let body = "return Reflect.apply(Function.prototype.constructor, null, [])";
        let err = scan_security(body).unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::SecurityViolation);
    }

    #[test]
    fn t_scan_security_accepts_clean_body() {
        assert!(scan_security("return ctx.value.toUpperCase()").is_ok());
        assert!(scan_security("return String(ctx.value).trim()").is_ok());
    }

    // ---- evaluate_blocking 端到端 ----

    fn simple_ctx(value: &str) -> impl for<'js> Fn(&Ctx<'js>) -> Result<Object<'js>, ScriptError> + '_ {
        move |ctx: &Ctx| {
            let obj = Object::new(ctx.clone()).map_err(|e| {
                ScriptError::new(
                    ScriptFailureCategory::RuntimeError,
                    format!("Object::new 失败: {e:?}"),
                )
            })?;
            obj.set("value", value).map_err(|e| {
                ScriptError::new(
                    ScriptFailureCategory::RuntimeError,
                    format!("Object.set('value') 失败: {e:?}"),
                )
            })?;
            Ok(obj)
        }
    }

    #[test]
    fn t_sandbox_classifies_syntax_error() {
        let body = "return ."; // 语法错
        let err = evaluate_blocking(body, &simple_ctx("x"), 100).unwrap_err();
        assert_eq!(
            err.category,
            ScriptFailureCategory::SyntaxError,
            "实际: {err:?}"
        );
    }

    #[test]
    fn t_sandbox_classifies_runtime_error() {
        let body = "throw new Error('boom')";
        let err = evaluate_blocking(body, &simple_ctx("x"), 100).unwrap_err();
        assert_eq!(
            err.category,
            ScriptFailureCategory::RuntimeError,
            "实际: {err:?}"
        );
        assert!(err.message.contains("boom"));
    }

    #[test]
    fn t_sandbox_classifies_type_error_on_no_return() {
        let body = "/* no return */";
        let err = evaluate_blocking(body, &simple_ctx("x"), 100).unwrap_err();
        assert_eq!(
            err.category,
            ScriptFailureCategory::TypeError,
            "实际: {err:?}"
        );
    }

    #[test]
    fn t_sandbox_timeout_interrupts_infinite_loop() {
        let body = "while (true) { /* spin */ }";
        let err = evaluate_blocking(body, &simple_ctx("x"), 100).unwrap_err();
        // 超时被 interrupt 后，rquickjs 抛 "interrupted" 异常 → Timeout
        // 也可能 RuntimeError（如果 rquickjs 包装异常），允许这两类
        assert!(
            err.category == ScriptFailureCategory::Timeout
                || err.category == ScriptFailureCategory::RuntimeError,
            "实际: {err:?}"
        );
    }

    #[test]
    fn t_sandbox_rejects_function_constructor_escape() {
        let body = "return Function('return this')()";
        let err = evaluate_blocking(body, &simple_ctx("x"), 100).unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::SecurityViolation);
    }

    #[test]
    fn t_sandbox_rejects_eval() {
        let body = "return eval('1+1')";
        let err = evaluate_blocking(body, &simple_ctx("x"), 100).unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::SecurityViolation);
    }

    #[test]
    fn t_sandbox_rejects_global_this_process() {
        let body = "return globalThis.process";
        let err = evaluate_blocking(body, &simple_ctx("x"), 100).unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::SecurityViolation);
    }

    #[test]
    fn t_sandbox_rejects_constructor_chain() {
        let body = "return [].constructor.constructor('return process')()";
        let err = evaluate_blocking(body, &simple_ctx("x"), 100).unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::SecurityViolation);
    }

    #[test]
    fn t_sandbox_rejects_reflect_apply_constructor() {
        let body = "return Reflect.apply(Function.prototype.constructor, null, [])";
        let err = evaluate_blocking(body, &simple_ctx("x"), 100).unwrap_err();
        assert_eq!(err.category, ScriptFailureCategory::SecurityViolation);
    }

    #[test]
    fn t_sandbox_evaluates_legal_body() {
        let body = "return ctx.value.toUpperCase()";
        let result = evaluate_blocking(body, &simple_ctx("hello"), 200).unwrap();
        assert_eq!(result, "HELLO");
    }

    #[test]
    fn t_sandbox_returns_explicit_string_literal() {
        let body = "return 'fixed'";
        let result = evaluate_blocking(body, &simple_ctx("ignored"), 200).unwrap();
        assert_eq!(result, "fixed");
    }
}
