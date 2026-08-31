/**
 * 归一化图床域名：trim、去尾斜杠、无协议时补 https://
 *
 * 配置值缺协议（如 img.example.com）时，拼接的 {domain}/{file_id} 会被
 * 浏览器按相对路径解析到系统域名下（https://sys.example.com/img.example.com/...）。
 * 后端保存/推送侧已做同样归一化，此处兜底展示存量脏数据。
 */
export function normalizeImageDomain(domain: string | null | undefined): string {
  const d = (domain ?? '').trim().replace(/\/+$/, '')
  if (!d) return ''
  return /^https?:\/\//.test(d) ? d : `https://${d}`
}
