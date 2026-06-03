import { useEffect, useRef, useState } from 'react'

/**
 * 动态计算 Ant Design Table scroll.y 值
 *
 * 使用 ResizeObserver 监听容器高度变化，
 * 自动减去表头（~55px）和分页（~56px）高度，
 * 返回精确的滚动区域高度。
 *
 * @param hasPagination 是否有 AntD 内置分页（默认 true）
 * @returns containerRef - 绑定到包裹 Table 的 div 上
 * @returns scrollY - 传给 Table 的 scroll={{ y: scrollY }}
 */
export function useTableScrollY(hasPagination = true) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [scrollY, setScrollY] = useState(300)

  useEffect(() => {
    const el = containerRef.current
    if (!el) return

    const update = () => {
      const h = el.clientHeight
      // 表头约 55px + 分页约 56px + 额外安全间距 8px
      const tableHeader = 55
      const paginationHeight = hasPagination ? 56 : 0
      const gap = 8
      setScrollY(Math.max(h - tableHeader - paginationHeight - gap, 80))
    }

    const ro = new ResizeObserver(update)
    ro.observe(el)
    update()
    return () => ro.disconnect()
  }, [hasPagination])

  return { containerRef, scrollY }
}
