import dayjs, { Dayjs } from 'dayjs'
import utc from 'dayjs/plugin/utc'

dayjs.extend(utc)

/**
 * 后端 chrono::Utc::now().naive_utc() 写入的字段是 UTC naive 字符串
 * （如 "2026-07-05 05:22:37"），无时区标记。dayjs 默认按本地时区解析，
 * 直接 format 会少 8 小时（显示成 UTC 时间）。
 *
 * 本函数把 UTC naive 字符串按 UTC 解析，再转浏览器本地时区返回 Dayjs。
 */
export function utcToLocal(v: string | null | undefined): Dayjs | null {
  if (!v) return null
  const d = dayjs.utc(v)
  return d.isValid() ? d.local() : null
}

/** 格式化 UTC naive 字符串为本地时间字符串；空值返回空串 */
export function fmtUtc(v: string | null | undefined, fmt = 'MM-DD HH:mm:ss'): string {
  const d = utcToLocal(v)
  return d ? d.format(fmt) : ''
}
