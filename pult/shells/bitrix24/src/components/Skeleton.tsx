/**
 * Скелетон вместо спиннера — UX-контракт эпика (осмысленные пустые
 * состояния, скелетоны вместо индикаторов ожидания).
 */

export function Skeleton({ lines = 3 }: { lines?: number }) {
  return (
    <div className="skeleton" aria-hidden="true">
      {Array.from({ length: lines }, (_, i) => (
        <div key={i} className="skeleton__line" style={{ width: `${Math.max(0, 100 - i * 18)}%` }} />
      ))}
    </div>
  )
}
