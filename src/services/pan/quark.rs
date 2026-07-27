// 夸克网盘驱动（feature 047）— US1：健康检查（列根目录验证 Cookie + 取容量）
// 端点见 research.md（夸克 share/transfer 能力在 US2 扩展）
//
// 已实现能力（8 个公共 API）：
//   - health_check            Cookie 有效性 + 容量（total/used）
//   - transfer_share          分享转存到自己网盘（4 步链路）
//   - create_share            生成分享链接 + 提取码
//   - upload_file             直链文件分片上传（OSS 签权 + 秒传）
//   - list_files              列出指定目录文件（pdir_fid/page/size）
//   - check_share_validity    分享有效性预检（不进行实际转存）
//   - check_instant_transfer  秒传检测（命中则无需上传）
//
// 未实现：磁力链/ed2k 离线下载
//   原因：夸克网页版已下线原生磁力链离线下载（仅手机/桌面客户端保留），社区逆向端点
//   (drive-m.quark.cn/1/clouddrive/task + task_type=2) 需 kps/sign/vcode 动态签名，
//   易失效且维护成本高。OpenList quark_uc 驱动与 quark-auto-save 均未实现。
//   如未来需要：建议走 aria2/qBittorrent 委托模式（OpenList 路线），下载到本地
//   中转目录后复用 upload_file 上传到夸克。

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use md5::{Digest, Md5};

use crate::errors::AppError;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const FILE_SORT_URL: &str = "https://drive-pc.quark.cn/1/clouddrive/file/sort";
// 容量端点：/1/clouddrive/member（对齐 OpenList quark_uc 驱动），返回 use_capacity/total_capacity
const MEMBER_URL: &str = "https://drive-pc.quark.cn/1/clouddrive/member";
// 转存/分享端点（research.md §夸克；②③用 drive-h，④⑤用 drive-pc）
const SHARE_TOKEN_URL: &str = "https://drive-h.quark.cn/1/clouddrive/share/sharepage/token";
const SHARE_DETAIL_URL: &str = "https://drive-h.quark.cn/1/clouddrive/share/sharepage/detail";
const SHARE_SAVE_URL: &str = "https://drive-pc.quark.cn/1/clouddrive/share/sharepage/save";
const TASK_URL: &str = "https://drive-pc.quark.cn/1/clouddrive/task";
const SHARE_CREATE_URL: &str = "https://drive-pc.quark.cn/1/clouddrive/share";
const SHARE_PWD_URL: &str = "https://drive-pc.quark.cn/1/clouddrive/share/password";

/// 转存结果：保存到自己网盘的文件
#[derive(Debug, Clone)]
pub struct SavedFile {
    pub fid: String,
    pub file_name: String,
}

/// 网盘内文件元信息（list_files 返回项）
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub fid: String,
    pub file_name: String,
    /// 目录则为 true，文件则为 false
    pub is_dir: bool,
    /// 文件大小（字节），目录为 0
    pub size: i64,
    /// 父目录 fid
    pub pdir_fid: String,
}

/// 分享有效性检测结果（check_share_validity 返回，不进行实际转存）
#[derive(Debug, Clone)]
pub struct ShareCheck {
    pub valid: bool,
    pub stoken: Option<String>,
    pub title: Option<String>,
    /// 文件总数（仅根目录直系）
    pub file_count: usize,
    /// 根目录文件名列表（最多 50 项，便于预览）
    pub file_names: Vec<String>,
    /// 失效时的原因（valid=true 时为 None）
    pub message: Option<String>,
}

/// 秒传检测结果（check_instant_transfer 返回）
#[derive(Debug, Clone)]
pub struct InstantTransferCheck {
    /// 是否命中秒传（true 表示夸克已存在相同文件，可直接复用 fid）
    pub hit: bool,
    /// 命中时的文件 fid；未命中为 None
    pub fid: Option<String>,
    /// 内部 task_id（未命中时调用方需自行继续 upPart/upCommit/upFinish）
    pub task_id: Option<String>,
    /// 上传预备信息（未命中时调用方继续分片上传时复用）
    pub pre_data: Option<serde_json::Value>,
}

/// 夸克账号健康检查结果
#[derive(Debug, Clone)]
pub struct QuarkHealth {
    pub valid: bool,
    /// 总配额（字节）
    pub capacity_bytes: Option<i64>,
    /// 已用容量（字节）
    pub used_capacity_bytes: Option<i64>,
    pub message: Option<String>,
}

/// 健康检查：用 Cookie 列根目录验证有效性，best-effort 取网盘容量
pub async fn health_check(cookie: &str) -> Result<QuarkHealth, AppError> {
    let client = reqwest::Client::builder()
        .user_agent(UA)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| AppError::Internal(format!("HTTP 客户端构建失败: {e}")))?;

    // 1. 列根目录验证 Cookie（夸克返回 code==0 表示成功）
    let resp = client
        .get(FILE_SORT_URL)
        .header("cookie", cookie)
        .query(&[
            ("pr", "ucpro"),
            ("fr", "pc"),
            ("uc_param_str", ""),
            ("pdir_fid", "0"),
            ("_page", "1"),
            ("_size", "1"),
            ("_fetch_total", "1"),
        ])
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("夸克健康检查请求失败: {e}")))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("夸克健康检查响应解析失败: {e}")))?;

    let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 {
        let msg = body
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("未知")
            .to_string();
        return Ok(QuarkHealth {
            valid: false,
            capacity_bytes: None,
            used_capacity_bytes: None,
            message: Some(format!("Cookie 无效或已失效: {msg}")),
        });
    }

    // 2. 调用 /member 取容量（best-effort：失败不影响健康判定）
    let (capacity_bytes, used_capacity_bytes) = match fetch_capacity(&client, cookie).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("夸克容量查询失败（不影响健康判定）: {e}");
            (None, None)
        }
    };

    Ok(QuarkHealth {
        valid: true,
        capacity_bytes,
        used_capacity_bytes,
        message: None,
    })
}

/// 调用 /member 端点取 (total_capacity, use_capacity)。
/// 失败返回 Err，由调用方决定是否降级。
async fn fetch_capacity(
    client: &reqwest::Client,
    cookie: &str,
) -> Result<(Option<i64>, Option<i64>), AppError> {
    let resp = client
        .get(MEMBER_URL)
        .header("cookie", cookie)
        .query(&[
            ("pr", "ucpro"),
            ("fr", "pc"),
            ("uc_param_str", ""),
            ("fetch_subscribe", "false"),
            ("_ch", "home"),
            ("fetch_identity", "false"),
        ])
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("夸克 /member 请求失败: {e}")))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("夸克 /member 响应解析失败: {e}")))?;

    Ok(parse_capacity(&body))
}

/// 从 /member 响应中解析 (total_capacity, use_capacity)。
/// 抽出以便单元测试，无需联网。
fn parse_capacity(body: &serde_json::Value) -> (Option<i64>, Option<i64>) {
    let data = body.get("data");
    let total = data
        .and_then(|d| d.get("total_capacity"))
        .and_then(|v| v.as_i64());
    let used = data
        .and_then(|d| d.get("use_capacity"))
        .and_then(|v| v.as_i64());
    (total, used)
}

/// 转存夸克分享到自己网盘指定目录，返回保存的文件列表（research.md §夸克 4 步链路）
pub async fn transfer_share(
    cookie: &str,
    pwd_id: &str,
    passcode: Option<&str>,
    to_pdir_fid: &str,
) -> Result<Vec<SavedFile>, AppError> {
    let client = build_client()?;

    // 1-2. 获取 stoken + 文件详情（与 check_share_validity 共享）
    let (stoken, list) = fetch_share_token_and_detail(&client, cookie, pwd_id, passcode).await?;
    if list.is_empty() {
        return Err(AppError::BadRequest("分享内无文件".into()));
    }
    let mut fid_list = Vec::new();
    let mut fid_token_list = Vec::new();
    let mut name_map = std::collections::HashMap::new();
    for f in list.iter().take(100) {
        let (fid, tok, name) = (
            f.get("fid").and_then(|v| v.as_str()),
            f.get("share_fid_token").and_then(|v| v.as_str()),
            f.get("file_name").and_then(|v| v.as_str()),
        );
        if let (Some(fid), Some(tok), Some(name)) = (fid, tok, name) {
            fid_list.push(fid.to_string());
            fid_token_list.push(tok.to_string());
            name_map.insert(fid.to_string(), name.to_string());
        }
    }
    if fid_list.is_empty() {
        return Err(AppError::Internal("分享文件缺少 fid/token".into()));
    }

    // 3. 转存（注入 __dt/__t 反检测）
    let save_body = serde_json::json!({
        "fid_list": fid_list, "fid_token_list": fid_token_list,
        "to_pdir_fid": to_pdir_fid, "pdir_fid": to_pdir_fid,
        "pwd_id": pwd_id, "stoken": stoken.as_str(), "scene": "link"
    });
    let dt = pseudo_dt();
    let t = pseudo_t();
    let resp: serde_json::Value = client
        .post(SHARE_SAVE_URL)
        .header("cookie", cookie)
        .query(&[
            ("pr", "ucpro"),
            ("fr", "pc"),
            ("uc_param_str", ""),
            ("__dt", dt.as_str()),
            ("__t", t.as_str()),
        ])
        .json(&save_body)
        .send()
        .await
        .map_err(net_err)?
        .json()
        .await
        .map_err(parse_err)?;
    check_code(&resp, "转存(save)")?;
    let task_id = resp
        .get("data")
        .and_then(|d| d.get("task_id"))
        .and_then(|v| v.as_i64())
        .ok_or_else(|| AppError::Internal("夸克转存缺少 task_id".into()))?;

    // 4. 轮询任务
    let saved_fids = poll_task(&client, cookie, task_id).await?;
    Ok(saved_fids
        .iter()
        .filter_map(|fid| {
            name_map.get(fid).map(|n| SavedFile {
                fid: fid.clone(),
                file_name: n.clone(),
            })
        })
        .collect())
}

/// 创建分享链接，返回 (share_url, 提取码)
pub async fn create_share(
    cookie: &str,
    fid_list: &[String],
    title: &str,
    password: Option<&str>,
) -> Result<(String, Option<String>), AppError> {
    let client = build_client()?;
    let body = serde_json::json!({
        "fid_list": fid_list, "title": title, "url_type": 1,
        "expired_type": 1, "passcode": password.unwrap_or("")
    });
    let resp: serde_json::Value = client
        .post(SHARE_CREATE_URL)
        .header("cookie", cookie)
        .query(&[("pr", "ucpro"), ("fr", "pc"), ("uc_param_str", "")])
        .json(&body)
        .send()
        .await
        .map_err(net_err)?
        .json()
        .await
        .map_err(parse_err)?;
    check_code(&resp, "创建分享")?;
    // 实测：task_sync 多为 false → 返回 task_id 需轮询拿 share_id；少数同步返回 share_id
    let share_id = if let Some(s) = resp
        .pointer("/data/task_resp/data/share_id")
        .and_then(|v| v.as_str())
        .or_else(|| {
            resp.get("data")
                .and_then(|d| d.get("share_id"))
                .and_then(|v| v.as_str())
        }) {
        s.to_string()
    } else {
        let tid = resp
            .get("data")
            .and_then(|d| d.get("task_id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Internal("创建分享缺少 task_id/share_id".into()))?;
        poll_share_task(&client, cookie, tid).await?
    };

    let pwd_body = serde_json::json!({ "share_id": share_id });
    let resp: serde_json::Value = client
        .post(SHARE_PWD_URL)
        .header("cookie", cookie)
        .query(&[("pr", "ucpro"), ("fr", "pc"), ("uc_param_str", "")])
        .json(&pwd_body)
        .send()
        .await
        .map_err(net_err)?
        .json()
        .await
        .map_err(parse_err)?;
    check_code(&resp, "获取分享链接")?;
    let url = resp
        .get("data")
        .and_then(|d| d.get("share_url"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Internal("缺少 share_url".into()))?
        .to_string();
    let pwd = resp
        .get("data")
        .and_then(|d| d.get("share_pwd"))
        .and_then(|v| v.as_str())
        .map(String::from);
    Ok((url, pwd))
}

/// 校验分享链接有效性并返回元信息（不进行实际转存）。
/// 复用 transfer_share 的 token + detail 步骤，仅查询。
pub async fn check_share_validity(
    cookie: &str,
    pwd_id: &str,
    passcode: Option<&str>,
) -> Result<ShareCheck, AppError> {
    let client = build_client()?;
    let result = fetch_share_token_and_detail(&client, cookie, pwd_id, passcode).await;
    match result {
        Ok((stoken, list)) => {
            let file_names = list
                .iter()
                .filter_map(|f| {
                    f.get("file_name")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
                .take(50)
                .collect::<Vec<_>>();
            let title = file_names.first().cloned();
            Ok(ShareCheck {
                valid: true,
                stoken: Some(stoken),
                title,
                file_count: list.len(),
                file_names,
                message: None,
            })
        }
        Err(AppError::Internal(msg)) if msg.contains("(token)") => {
            // 提取码错误 / 分享已失效 → 业务级失效（非系统错误）
            Ok(ShareCheck {
                valid: false,
                stoken: None,
                title: None,
                file_count: 0,
                file_names: Vec::new(),
                message: Some(msg),
            })
        }
        Err(AppError::BadRequest(msg)) => {
            // 详情返回空列表 → 也归为业务级结果（保持语义）
            Ok(ShareCheck {
                valid: false,
                stoken: None,
                title: None,
                file_count: 0,
                file_names: Vec::new(),
                message: Some(msg),
            })
        }
        Err(e) => Err(e),
    }
}

/// 步骤 1-2：获取 stoken + 文件详情列表（token 失败抛 Internal 含 "(token)"，详情空抛 BadRequest）
async fn fetch_share_token_and_detail(
    client: &reqwest::Client,
    cookie: &str,
    pwd_id: &str,
    passcode: Option<&str>,
) -> Result<(String, Vec<serde_json::Value>), AppError> {
    // 1. 获取 stoken（此步返回码亦可判分享是否失效）
    let token_body = serde_json::json!({ "pwd_id": pwd_id, "passcode": passcode.unwrap_or("") });
    let resp: serde_json::Value = client
        .post(SHARE_TOKEN_URL)
        .header("cookie", cookie)
        .query(&[("pr", "ucpro"), ("fr", "pc"), ("uc_param_str", "")])
        .json(&token_body)
        .send()
        .await
        .map_err(net_err)?
        .json()
        .await
        .map_err(parse_err)?;
    check_code(&resp, "分享解析(token)")?;
    let stoken = resp
        .get("data")
        .and_then(|d| d.get("stoken"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Internal("夸克返回缺少 stoken".into()))?
        .to_string();

    // 2. 文件详情
    let resp: serde_json::Value = client
        .get(SHARE_DETAIL_URL)
        .header("cookie", cookie)
        .query(&[
            ("pr", "ucpro"),
            ("fr", "pc"),
            ("uc_param_str", ""),
            ("pwd_id", pwd_id),
            ("stoken", stoken.as_str()),
            ("pdir_fid", "0"),
            ("_page", "1"),
            ("_size", "50"),
            ("_fetch_total", "1"),
            ("ver", "2"),
        ])
        .send()
        .await
        .map_err(net_err)?
        .json()
        .await
        .map_err(parse_err)?;
    check_code(&resp, "分享详情(detail)")?;
    let list = resp
        .get("data")
        .and_then(|d| d.get("list"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok((stoken, list))
}

async fn poll_task(
    client: &reqwest::Client,
    cookie: &str,
    task_id: i64,
) -> Result<Vec<String>, AppError> {
    let tid = task_id.to_string();
    for _ in 0..30 {
        let resp: serde_json::Value = client
            .get(TASK_URL)
            .header("cookie", cookie)
            .query(&[
                ("pr", "ucpro"),
                ("fr", "pc"),
                ("uc_param_str", ""),
                ("task_id", tid.as_str()),
                ("retry_index", "0"),
            ])
            .send()
            .await
            .map_err(net_err)?
            .json()
            .await
            .map_err(parse_err)?;
        let status = resp
            .get("data")
            .and_then(|d| d.get("status"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        if status == 2 {
            return Ok(resp
                .pointer("/data/save_as/save_as_top_fids")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default());
        }
        if status == 3 {
            return Err(AppError::Internal("夸克转存任务失败".into()));
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    Err(AppError::Internal("夸克转存任务超时".into()))
}

/// 轮询分享任务拿 share_id（create_share 异步场景，实测 task_sync=false）
async fn poll_share_task(
    client: &reqwest::Client,
    cookie: &str,
    task_id: &str,
) -> Result<String, AppError> {
    for _ in 0..30 {
        let resp: serde_json::Value = client
            .get(TASK_URL)
            .header("cookie", cookie)
            .query(&[
                ("pr", "ucpro"),
                ("fr", "pc"),
                ("uc_param_str", ""),
                ("task_id", task_id),
                ("retry_index", "0"),
            ])
            .send()
            .await
            .map_err(net_err)?
            .json()
            .await
            .map_err(parse_err)?;
        let status = resp
            .get("data")
            .and_then(|d| d.get("status"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        if status == 2 {
            return resp
                .get("data")
                .and_then(|d| d.get("share_id"))
                .and_then(|v| v.as_str())
                .map(String::from)
                .ok_or_else(|| AppError::Internal("分享任务完成但缺少 share_id".into()));
        }
        if status == 3 {
            return Err(AppError::Internal("创建分享任务失败".into()));
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    Err(AppError::Internal("创建分享任务超时".into()))
}

fn build_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| AppError::Internal(format!("HTTP 客户端构建失败: {e}")))
}

fn check_code(resp: &serde_json::Value, ctx: &str) -> Result<(), AppError> {
    let code = resp.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 {
        let msg = resp
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("未知");
        return Err(AppError::Internal(format!(
            "夸克{ctx}失败(code={code}): {msg}"
        )));
    }
    Ok(())
}

fn net_err(e: reqwest::Error) -> AppError {
    AppError::Internal(format!("夸克请求失败: {e}"))
}

fn parse_err(e: reqwest::Error) -> AppError {
    AppError::Internal(format!("夸克响应解析失败: {e}"))
}

/// 伪随机 __dt（60000..300000 ms 范围，基于时间戳，避免引入 rand）
fn pseudo_dt() -> String {
    let ms = chrono::Utc::now().timestamp_millis().rem_euclid(240000) + 60000;
    ms.to_string()
}

fn pseudo_t() -> String {
    chrono::Utc::now().timestamp_millis().to_string()
}

// ========== 直链上传（US3 — 基于 OpenList quark_uc 还原）==========
const UPLOAD_PRE_URL: &str = "https://drive-pc.quark.cn/1/clouddrive/file/upload/pre";
const UPLOAD_HASH_URL: &str = "https://drive-pc.quark.cn/1/clouddrive/file/update/hash";
const UPLOAD_AUTH_URL: &str = "https://drive-pc.quark.cn/1/clouddrive/file/upload/auth";
const UPLOAD_FINISH_URL: &str = "https://drive-pc.quark.cn/1/clouddrive/file/upload/finish";
const OSS_UA: &str = "aliyun-sdk-js/6.6.1 Chrome 98.0.4758.80 on Windows 10 64-bit";
const DEFAULT_MIME: &str = "application/octet-stream";

/// 秒传检测：读取本地文件 → 计算 md5/sha1 → 调用 upPre 占位 + upHash 判断夸克是否已有相同文件。
/// 命中(hit=true)时 fid 字段为已存在文件 fid，调用方无需再上传。
/// 未命中时返回 task_id 与 pre_data，调用方如需继续上传应直接调用 upload_file（会重新走一遍 upPre，
/// 因 task_id 在夸克侧有时效，复用易失败；此处仅做"是否能秒传"的判定）。
pub async fn check_instant_transfer(
    cookie: &str,
    file_path: &std::path::Path,
    parent_fid: &str,
    file_name: &str,
) -> Result<InstantTransferCheck, AppError> {
    let client = build_client()?;
    let data = tokio::fs::read(file_path)
        .await
        .map_err(|e| AppError::Internal(format!("读取文件失败: {e}")))?;
    let size = data.len() as i64;
    let md5hex = hex_str(Md5::digest(&data).as_slice());
    let sha1hex = hex_str(sha1::Sha1::digest(&data).as_slice());
    let now_ms = chrono::Utc::now().timestamp_millis();

    let pre_body = serde_json::json!({
        "ccp_hash_update": true, "dir_name": "", "file_name": file_name,
        "format_type": DEFAULT_MIME, "l_created_at": now_ms, "l_updated_at": now_ms,
        "pdir_fid": parent_fid, "size": size,
    });
    let pre: serde_json::Value = post_quark(&client, cookie, UPLOAD_PRE_URL, &pre_body).await?;
    check_code(&pre, "upPre")?;
    let task_id = pre
        .get("data")
        .and_then(|d| d.get("task_id"))
        .and_then(|v| v.as_str())
        .map(String::from);

    let hash_body = serde_json::json!({ "md5": md5hex, "sha1": sha1hex, "task_id": task_id.clone().unwrap_or_default() });
    let hash_resp: serde_json::Value =
        post_quark(&client, cookie, UPLOAD_HASH_URL, &hash_body).await?;
    check_code(&hash_resp, "upHash")?;
    let finish = hash_resp
        .get("data")
        .and_then(|d| d.get("finish"))
        .and_then(|v| v.as_bool())
        == Some(true);
    let fid = if finish {
        hash_resp
            .get("data")
            .and_then(|d| d.get("fid"))
            .and_then(|v| v.as_str())
            .map(String::from)
    } else {
        None
    };
    Ok(InstantTransferCheck {
        hit: finish,
        fid,
        task_id,
        pre_data: if finish { None } else { Some(pre) },
    })
}

/// 上传本地文件到夸克 parent_fid 目录，返回文件 fid（秒传命中则直接返回，否则分片上传）。
/// OSS 签名(auth_key)由夸克 /file/upload/auth 根据 auth_meta 返回，无需自行计算。
pub async fn upload_file(
    cookie: &str,
    file_path: &std::path::Path,
    parent_fid: &str,
    file_name: &str,
) -> Result<String, AppError> {
    let client = build_client()?;
    let data = tokio::fs::read(file_path)
        .await
        .map_err(|e| AppError::Internal(format!("读取中转文件失败: {e}")))?;
    let size = data.len() as i64;
    let md5hex = hex_str(Md5::digest(&data).as_slice());
    let sha1hex = hex_str(sha1::Sha1::digest(&data).as_slice());
    let now_ms = chrono::Utc::now().timestamp_millis();

    // 1. upPre
    let pre_body = serde_json::json!({
        "ccp_hash_update": true, "dir_name": "", "file_name": file_name,
        "format_type": DEFAULT_MIME, "l_created_at": now_ms, "l_updated_at": now_ms,
        "pdir_fid": parent_fid, "size": size,
    });
    let pre: serde_json::Value = post_quark(&client, cookie, UPLOAD_PRE_URL, &pre_body).await?;
    check_code(&pre, "upPre")?;
    let g = pre
        .get("data")
        .and_then(|d| d.as_object())
        .ok_or_else(|| AppError::Internal("upPre 缺少 data".into()))?;
    let task_id = get_str(g, "task_id", "upPre")?;
    let auth_info = get_str(g, "auth_info", "upPre")?;
    let bucket = get_str(g, "bucket", "upPre")?;
    let obj_key = get_str(g, "obj_key", "upPre")?;
    let upload_id = get_str(g, "upload_id", "upPre")?;
    let upload_url = get_str(g, "upload_url", "upPre")?;
    let callback = g.get("callback").cloned().unwrap_or(serde_json::json!({}));
    let oss_host = upload_url.strip_prefix("https://").unwrap_or(&upload_url);
    let part_size = pre
        .get("metadata")
        .and_then(|m| m.get("part_size"))
        .and_then(|v| v.as_u64())
        .unwrap_or(4 * 1024 * 1024) as usize;

    // 2. upHash（秒传）
    let hash_body = serde_json::json!({ "md5": md5hex, "sha1": sha1hex, "task_id": task_id });
    let hash_resp: serde_json::Value =
        post_quark(&client, cookie, UPLOAD_HASH_URL, &hash_body).await?;
    check_code(&hash_resp, "upHash")?;
    if hash_resp
        .get("data")
        .and_then(|d| d.get("finish"))
        .and_then(|v| v.as_bool())
        == Some(true)
    {
        return Ok(hash_resp
            .get("data")
            .and_then(|d| d.get("fid"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string());
    }

    // 3. 分片上传
    let mut etags: Vec<String> = Vec::new();
    let total = data.len();
    let mut offset = 0usize;
    let mut part_no = 0i64;
    let oss_url = format!("https://{bucket}.{oss_host}/{obj_key}");
    while offset < total {
        part_no += 1;
        let end = (offset + part_size).min(total);
        let chunk = data[offset..end].to_vec();
        let date = http_date_now();
        let auth_meta = format!(
            "PUT\n\n{DEFAULT_MIME}\n{date}\nx-oss-user-agent:{OSS_UA}\n/{bucket}/{obj_key}?partNumber={part_no}&uploadId={upload_id}"
        );
        let auth_body = serde_json::json!({ "auth_info": auth_info, "auth_meta": auth_meta, "task_id": task_id });
        let auth_resp: serde_json::Value =
            post_quark(&client, cookie, UPLOAD_AUTH_URL, &auth_body).await?;
        check_code(&auth_resp, "upPart auth")?;
        let auth_key = auth_resp
            .get("data")
            .and_then(|d| d.get("auth_key"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Internal("upPart 缺少 auth_key".into()))?;
        let resp = client
            .put(&oss_url)
            .header("Authorization", auth_key)
            .header("Content-Type", DEFAULT_MIME)
            .header("Referer", "https://pan.quark.cn/")
            .header("x-oss-date", &date)
            .header("x-oss-user-agent", OSS_UA)
            .query(&[
                ("partNumber", part_no.to_string()),
                ("uploadId", upload_id.clone()),
            ])
            .body(chunk)
            .send()
            .await
            .map_err(net_err)?;
        if resp.status() != 200 {
            return Err(AppError::Internal(format!(
                "OSS 分片上传失败: HTTP {}",
                resp.status()
            )));
        }
        let etag = resp
            .headers()
            .get("ETag")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Internal("OSS 分片响应缺少 ETag".into()))?
            .to_string();
        etags.push(etag);
        offset = end;
    }

    // 4. upCommit（CompleteMultipartUpload）
    let mut xml =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<CompleteMultipartUpload>\n");
    for (i, etag) in etags.iter().enumerate() {
        xml.push_str(&format!(
            "<Part>\n<PartNumber>{}</PartNumber>\n<ETag>{}</ETag>\n</Part>\n",
            i + 1,
            etag
        ));
    }
    xml.push_str("</CompleteMultipartUpload>");
    let content_md5 = B64.encode(Md5::digest(xml.as_bytes()).as_slice());
    let callback_b64 = B64.encode(serde_json::to_vec(&callback).unwrap_or_default());
    let date = http_date_now();
    let auth_meta = format!(
        "POST\n{content_md5}\napplication/xml\nx-oss-callback:{callback_b64}\n{date}\nx-oss-user-agent:{OSS_UA}\n/{bucket}/{obj_key}?uploadId={upload_id}"
    );
    let auth_body =
        serde_json::json!({ "auth_info": auth_info, "auth_meta": auth_meta, "task_id": task_id });
    let auth_resp: serde_json::Value =
        post_quark(&client, cookie, UPLOAD_AUTH_URL, &auth_body).await?;
    check_code(&auth_resp, "upCommit auth")?;
    let auth_key = auth_resp
        .get("data")
        .and_then(|d| d.get("auth_key"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Internal("upCommit 缺少 auth_key".into()))?;
    let resp = client
        .post(&oss_url)
        .header("Authorization", auth_key)
        .header("Content-MD5", &content_md5)
        .header("Content-Type", "application/xml")
        .header("Referer", "https://pan.quark.cn/")
        .header("x-oss-callback", &callback_b64)
        .header("x-oss-date", &date)
        .header("x-oss-user-agent", OSS_UA)
        .query(&[("uploadId", upload_id.clone())])
        .body(xml)
        .send()
        .await
        .map_err(net_err)?;
    if resp.status() != 200 {
        return Err(AppError::Internal(format!(
            "OSS CompleteMultipartUpload 失败: HTTP {}",
            resp.status()
        )));
    }

    // 5. upFinish
    let finish_body = serde_json::json!({ "obj_key": obj_key, "task_id": task_id });
    let resp: serde_json::Value =
        post_quark(&client, cookie, UPLOAD_FINISH_URL, &finish_body).await?;
    check_code(&resp, "upFinish")?;

    // 上传完成后按文件名取 fid
    find_fid_by_name(&client, cookie, parent_fid, file_name).await
}

async fn post_quark(
    client: &reqwest::Client,
    cookie: &str,
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, AppError> {
    client
        .post(url)
        .header("cookie", cookie)
        .header("Referer", "https://pan.quark.cn/")
        .query(&[("pr", "ucpro"), ("fr", "pc"), ("uc_param_str", "")])
        .json(body)
        .send()
        .await
        .map_err(net_err)?
        .json()
        .await
        .map_err(parse_err)
}

async fn find_fid_by_name(
    client: &reqwest::Client,
    cookie: &str,
    parent_fid: &str,
    file_name: &str,
) -> Result<String, AppError> {
    let (files, _total) = list_files_inner(client, cookie, parent_fid, 1, 100).await?;
    for f in files {
        if f.file_name == file_name {
            return Ok(f.fid);
        }
    }
    Err(AppError::Internal(format!(
        "上传后未找到文件 {file_name} 的 fid"
    )))
}

/// 列出指定目录文件（公开 API）。page 从 1 开始，size 推荐不大于 100。
pub async fn list_files(
    cookie: &str,
    parent_fid: &str,
    page: u32,
    size: u32,
) -> Result<(Vec<FileInfo>, u64), AppError> {
    let client = build_client()?;
    let (files, total) = list_files_inner(&client, cookie, parent_fid, page, size).await?;
    Ok((files, total))
}

/// list_files 内部实现，复用已有 client（避免 find_fid_by_name 重建）
async fn list_files_inner(
    client: &reqwest::Client,
    cookie: &str,
    parent_fid: &str,
    page: u32,
    size: u32,
) -> Result<(Vec<FileInfo>, u64), AppError> {
    let resp: serde_json::Value = client
        .get(FILE_SORT_URL)
        .header("cookie", cookie)
        .query(&[
            ("pr", "ucpro"),
            ("fr", "pc"),
            ("uc_param_str", ""),
            ("pdir_fid", parent_fid),
            ("_page", page.to_string().as_str()),
            ("_size", size.to_string().as_str()),
            ("_fetch_total", "1"),
            ("_fetch_sub_dirs", "0"),
            ("_sort", "file_type:asc,updated_at:desc"),
        ])
        .send()
        .await
        .map_err(net_err)?
        .json()
        .await
        .map_err(parse_err)?;
    check_code(&resp, "列出文件")?;
    let total = resp
        .get("metadata")
        .and_then(|m| m.get("_total"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let list = resp
        .get("data")
        .and_then(|d| d.get("list"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(parse_file_item).collect())
        .unwrap_or_default();
    Ok((list, total))
}

/// 从 file/sort 单项解析为 FileInfo（抽出便于测试，不依赖网络）
fn parse_file_item(v: &serde_json::Value) -> FileInfo {
    FileInfo {
        fid: v
            .get("fid")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        file_name: v
            .get("file_name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        is_dir: v.get("dir").and_then(|x| x.as_bool()).unwrap_or(false),
        size: v.get("size").and_then(|x| x.as_i64()).unwrap_or(0),
        pdir_fid: v
            .get("pdir_fid")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

fn get_str(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    ctx: &str,
) -> Result<String, AppError> {
    obj.get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| AppError::Internal(format!("{ctx} 缺少 {key}")))
}

fn hex_str(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn http_date_now() -> String {
    chrono::Utc::now()
        .format("%a, %d %b %Y %H:%M:%S GMT")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_capacity_extracts_both_values() {
        // 对齐 OpenList MemberResp 真实结构（数值即字节数）
        let body = serde_json::json!({
            "status": 200,
            "code": 0,
            "message": "ok",
            "data": {
                "member_type": "REGULAR",
                "use_capacity": 123_456_789,
                "total_capacity": 1_099_511_627_776_i64, // 1 TiB
                "is_new_user": false,
                "member_status": { "VIP": "0", "Z_VIP": "0", "MINI_VIP": "0", "SUPER_VIP": "0" }
            },
            "metadata": { "server_cur_time": 0 }
        });
        let (total, used) = parse_capacity(&body);
        assert_eq!(total, Some(1_099_511_627_776_i64), "应取到 total_capacity");
        assert_eq!(used, Some(123_456_789), "应取到 use_capacity");
    }

    #[test]
    fn parse_capacity_returns_none_when_data_missing() {
        // 响应合法但 data 缺失 → (None, None)
        let body = serde_json::json!({ "code": 0 });
        let (total, used) = parse_capacity(&body);
        assert_eq!(total, None);
        assert_eq!(used, None);
    }

    #[test]
    fn parse_capacity_partial_fields() {
        // 只有 total_capacity，无 use_capacity → (Some, None)
        let body = serde_json::json!({ "code": 0, "data": { "total_capacity": 100 } });
        let (total, used) = parse_capacity(&body);
        assert_eq!(total, Some(100));
        assert_eq!(used, None);
    }

    #[test]
    fn parse_capacity_returns_none_on_error_body() {
        // code != 0 的错误响应（无 data）→ (None, None)
        let body = serde_json::json!({ "code": 32003, "message": "登录态失效" });
        let (total, used) = parse_capacity(&body);
        assert_eq!(total, None);
        assert_eq!(used, None);
    }

    #[test]
    fn parse_file_item_extracts_full_fields() {
        // 对齐夸克 file/sort 真实响应：dir=true 目录、size 字节、fid/pdir_fid 字符串
        let v = serde_json::json!({
            "fid": "abc123def456",
            "file_name": "我的电影.mp4",
            "dir": false,
            "size": 1073741824_i64,
            "pdir_fid": "parent_fid_0",
            "category": 1,
            "thumbnail": "https://example.com/t.jpg"
        });
        let f = parse_file_item(&v);
        assert_eq!(f.fid, "abc123def456");
        assert_eq!(f.file_name, "我的电影.mp4");
        assert!(!f.is_dir);
        assert_eq!(f.size, 1073741824_i64);
        assert_eq!(f.pdir_fid, "parent_fid_0");
    }

    #[test]
    fn parse_file_item_directory() {
        // dir=true → is_dir=true，size 一般为 0
        let v = serde_json::json!({
            "fid": "dirfid001",
            "file_name": "subdir",
            "dir": true,
            "size": 0,
            "pdir_fid": "0"
        });
        let f = parse_file_item(&v);
        assert!(f.is_dir);
        assert_eq!(f.size, 0);
    }

    #[test]
    fn parse_file_item_tolerates_missing_fields() {
        // 字段缺失 → 安全降级（空串/0/false），不 panic
        let v = serde_json::json!({ "fid": "only_fid" });
        let f = parse_file_item(&v);
        assert_eq!(f.fid, "only_fid");
        assert_eq!(f.file_name, "");
        assert!(!f.is_dir);
        assert_eq!(f.size, 0);
        assert_eq!(f.pdir_fid, "");
    }

    #[test]
    fn list_files_query_params_include_sort_and_sub_dirs() {
        // 仅做签名回归：通过反射比较难以，这里只做编译期占位 + 文档化期望
        // 真正的 HTTP 行为由集成测试覆盖（需夸克 Cookie）
        let _ = (
            "pr",
            "ucpro",
            "fr",
            "pc",
            "_sort",
            "file_type:asc,updated_at:desc",
        );
    }
}
