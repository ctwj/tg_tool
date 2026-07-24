// 夸克网盘驱动（feature 047）— US1：健康检查（列根目录验证 Cookie + 取容量）
// 端点见 research.md（夸克 share/transfer 能力在 US2 扩展）

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use md5::{Digest, Md5};

use crate::errors::AppError;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const FILE_SORT_URL: &str = "https://drive-pc.quark.cn/1/clouddrive/file/sort";
// 注：/1/clouddrive/capacity 实测返回 404，容量端点待确认；health_check 暂不取容量数值
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

/// 夸克账号健康检查结果
#[derive(Debug, Clone)]
pub struct QuarkHealth {
    pub valid: bool,
    pub capacity_bytes: Option<i64>,
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
            message: Some(format!("Cookie 无效或已失效: {msg}")),
        });
    }

    // 2. 容量端点待确认（/capacity 实测 404）；暂返回 None，不影响健康判定
    let capacity_bytes: Option<i64> = None;

    Ok(QuarkHealth {
        valid: true,
        capacity_bytes,
        message: None,
    })
}

/// 转存夸克分享到自己网盘指定目录，返回保存的文件列表（research.md §夸克 4 步链路）
pub async fn transfer_share(
    cookie: &str,
    pwd_id: &str,
    passcode: Option<&str>,
    to_pdir_fid: &str,
) -> Result<Vec<SavedFile>, AppError> {
    let client = build_client()?;

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
        .ok_or_else(|| AppError::Internal("夸克详情缺少文件列表".into()))?;
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
        .query(&[("pr", "ucpro"), ("fr", "pc"), ("uc_param_str", ""), ("__dt", dt.as_str()), ("__t", t.as_str())])
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
            name_map
                .get(fid)
                .map(|n| SavedFile { fid: fid.clone(), file_name: n.clone() })
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
        let msg = resp.get("message").and_then(|v| v.as_str()).unwrap_or("未知");
        return Err(AppError::Internal(format!("夸克{ctx}失败(code={code}): {msg}")));
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
    let hash_resp: serde_json::Value = post_quark(&client, cookie, UPLOAD_HASH_URL, &hash_body).await?;
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
        let auth_body =
            serde_json::json!({ "auth_info": auth_info, "auth_meta": auth_meta, "task_id": task_id });
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
            .query(&[("partNumber", part_no.to_string()), ("uploadId", upload_id.clone())])
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
    let resp: serde_json::Value = post_quark(&client, cookie, UPLOAD_FINISH_URL, &finish_body).await?;
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
    let resp: serde_json::Value = client
        .get(FILE_SORT_URL)
        .header("cookie", cookie)
        .query(&[
            ("pr", "ucpro"),
            ("fr", "pc"),
            ("uc_param_str", ""),
            ("pdir_fid", parent_fid),
            ("_page", "1"),
            ("_size", "100"),
            ("_fetch_total", "1"),
        ])
        .send()
        .await
        .map_err(net_err)?
        .json()
        .await
        .map_err(parse_err)?;
    if let Some(list) = resp
        .get("data")
        .and_then(|d| d.get("list"))
        .and_then(|v| v.as_array())
    {
        for f in list {
            if f.get("file_name").and_then(|v| v.as_str()) == Some(file_name)
                && let Some(fid) = f.get("fid").and_then(|v| v.as_str()) {
                    return Ok(fid.to_string());
                }
        }
    }
    Err(AppError::Internal(format!(
        "上传后未找到文件 {file_name} 的 fid"
    )))
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
