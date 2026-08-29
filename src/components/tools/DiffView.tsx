import { memo } from 'react';
import type { DiffHunk, DiffLine } from '@/lib/events';

/**
 * 彩色 diff 视图。
 *
 * hunks 由 Rust 侧用 `similar` 算好，前端只做染色 —— 不引任何 JS diff 库。
 * 行内高亮走片段拼接而非字符索引，避免 Rust 与 JS 的字符计数差异。
 */
export const DiffView = memo(function DiffView({
  path,
  hunks,
  added,
  removed,
}: {
  path: string;
  hunks: DiffHunk[];
  added: number;
  removed: number;
}) {
  return (
    <div>
      <div className="flex items-center gap-2 border-b border-(--border) px-3 py-1.5">
        <span className="flex-1 truncate font-mono text-[11.5px] text-(--fg-muted)">{path}</span>
        {added > 0 && (
          <span className="font-mono text-[11px] text-(--color-diff-add) tabular-nums">
            +{added}
          </span>
        )}
        {removed > 0 && (
          <span className="font-mono text-[11px] text-(--color-diff-del) tabular-nums">
            −{removed}
          </span>
        )}
      </div>

      <div className="max-h-96 overflow-auto">
        {hunks.map((hunk, i) => (
          <div key={i}>
            {i > 0 && (
              <div className="border-y border-(--border) bg-(--bg-inset) px-3 py-0.5 text-center font-mono text-[10px] text-(--fg-subtle)">
                ⋯
              </div>
            )}
            {hunk.lines.map((line, j) => (
              <Row key={j} line={line} />
            ))}
          </div>
        ))}
      </div>
    </div>
  );
});

function Row({ line }: { line: DiffLine }) {
  const style = ROW_STYLES[line.tag];

  return (
    <div className={`flex font-mono text-[12px] leading-[1.6] ${style.row}`}>
      <Gutter value={line.old_line} />
      <Gutter value={line.new_line} />

      <span className={`w-4 shrink-0 select-none text-center ${style.marker}`}>{style.sign}</span>

      <span className="selectable flex-1 whitespace-pre-wrap break-all pr-3">
        {line.segments.map((segment, i) =>
          segment.emphasis ? (
            <span key={i} className={style.emphasis}>
              {segment.text}
            </span>
          ) : (
            <span key={i}>{segment.text}</span>
          ),
        )}
      </span>
    </div>
  );
}

/** 行号栏。插入行没有原行号，删除行没有新行号，留空。 */
function Gutter({ value }: { value: number | null }) {
  return (
    <span className="w-11 shrink-0 select-none pr-2 text-right text-(--fg-subtle) opacity-60 tabular-nums">
      {value ?? ''}
    </span>
  );
}

/**
 * 行样式。
 *
 * 整行用极淡的底色标出增删，变化的片段再叠一层更实的底色 ——
 * 这样一眼能扫到哪些行动了，细看能定位到具体改了哪几个字符。
 */
const ROW_STYLES = {
  eq: {
    row: '',
    marker: 'text-(--fg-subtle) opacity-40',
    sign: '',
    emphasis: '',
  },
  ins: {
    row: 'bg-(--color-diff-add)/[0.08]',
    marker: 'text-(--color-diff-add)',
    sign: '+',
    emphasis: 'rounded-[2px] bg-(--color-diff-add)/25',
  },
  del: {
    row: 'bg-(--color-diff-del)/[0.08]',
    marker: 'text-(--color-diff-del)',
    sign: '−',
    emphasis: 'rounded-[2px] bg-(--color-diff-del)/25',
  },
} as const;
