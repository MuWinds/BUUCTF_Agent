import { memo, type ReactNode } from 'react';

/**
 * 助手正文渲染。
 *
 * 流式期间只识别围栏代码块，其余按纯文本保留换行 —— 流式时跑完整
 * Markdown + 语法高亮是同类应用掉帧的头号原因。完整 Markdown 只在
 * 轮次结束后对定稿文本做一次（`streaming=false` 时）。
 */

interface Block {
  kind: 'text' | 'code';
  lang?: string;
  content: string;
}

const FENCE = /^```([\w+-]*)\s*$/;

function parseLight(source: string): Block[] {
  const blocks: Block[] = [];
  let buffer: string[] = [];
  let inCode = false;
  let lang = '';

  const flush = () => {
    if (buffer.length === 0) return;
    blocks.push({
      kind: inCode ? 'code' : 'text',
      lang: inCode ? lang : undefined,
      content: buffer.join('\n'),
    });
    buffer = [];
  };

  for (const line of source.split('\n')) {
    const fence = FENCE.exec(line);
    if (fence) {
      flush();
      inCode = !inCode;
      lang = inCode ? (fence[1] ?? '') : '';
      continue;
    }
    buffer.push(line);
  }
  flush();

  return blocks;
}

export const RichText = memo(function RichText({
  text,
  streaming,
}: {
  text: string;
  streaming?: boolean;
}) {
  if (!text) return null;

  if (streaming) {
    const blocks = parseLight(text);
    return (
      <div className="selectable space-y-3">
        {blocks.map((block, i) =>
          block.kind === 'code' ? (
            <CodeBlock key={i} lang={block.lang} content={block.content} />
          ) : (
            <p key={i} className="whitespace-pre-wrap wrap-break-word leading-relaxed">
              {block.content}
            </p>
          ),
        )}
      </div>
    );
  }

  return <Markdown text={text} />;
});

function CodeBlock({ lang, content }: { lang?: string; content: string }) {
  return (
    <div className="overflow-hidden rounded-card border border-(--border) bg-(--bg-inset)">
      {lang && (
        <div className="border-b border-(--border) px-3 py-1.5 font-mono text-[11px] text-(--fg-subtle)">
          {lang}
        </div>
      )}
      <pre className="overflow-x-auto p-3">
        <code className="font-mono text-[13px] leading-relaxed">{content}</code>
      </pre>
    </div>
  );
}

// ---------------------------------------------------------------
// 完整 Markdown（仅在轮次结束后运行）
// ---------------------------------------------------------------

type Inline =
  | { kind: 'text'; text: string }
  | { kind: 'code'; text: string }
  | { kind: 'bold'; children: Inline[] }
  | { kind: 'italic'; children: Inline[] }
  | { kind: 'strike'; children: Inline[] }
  | { kind: 'link'; href: string; children: Inline[] };

type MdBlock =
  | { kind: 'code'; lang?: string; content: string }
  | { kind: 'heading'; level: number; content: Inline[] }
  | { kind: 'paragraph'; content: Inline[] }
  | { kind: 'quote'; content: Inline[] }
  | { kind: 'list'; ordered: boolean; items: Inline[][] }
  | {
      kind: 'table';
      headers: Inline[][];
      align: (null | 'left' | 'center' | 'right')[];
      rows: Inline[][][];
    }
  | { kind: 'hr' };

/** 把一段行内文本解析成节点。支持 `` ` ``、`**`、`*`、`~~`、`[text](url)`。 */
function parseInline(src: string): Inline[] {
  const out: Inline[] = [];
  let buf = '';
  const flush = () => {
    if (buf) {
      out.push({ kind: 'text', text: buf });
      buf = '';
    }
  };

  let i = 0;
  while (i < src.length) {
    const ch = src[i];

    if (ch === '`') {
      const end = src.indexOf('`', i + 1);
      if (end !== -1) {
        flush();
        out.push({ kind: 'code', text: src.slice(i + 1, end) });
        i = end + 1;
        continue;
      }
    }

    if (ch === '[') {
      const close = src.indexOf(']', i + 1);
      if (close !== -1 && src[close + 1] === '(') {
        const paren = src.indexOf(')', close + 2);
        if (paren !== -1) {
          flush();
          out.push({
            kind: 'link',
            href: src.slice(close + 2, paren),
            children: parseInline(src.slice(i + 1, close)),
          });
          i = paren + 1;
          continue;
        }
      }
    }

    if (ch === '*' && src[i + 1] === '*') {
      const end = src.indexOf('**', i + 2);
      if (end !== -1) {
        flush();
        out.push({ kind: 'bold', children: parseInline(src.slice(i + 2, end)) });
        i = end + 2;
        continue;
      }
    }

    if (ch === '~' && src[i + 1] === '~') {
      const end = src.indexOf('~~', i + 2);
      if (end !== -1) {
        flush();
        out.push({ kind: 'strike', children: parseInline(src.slice(i + 2, end)) });
        i = end + 2;
        continue;
      }
    }

    if (ch === '*') {
      const end = src.indexOf('*', i + 1);
      if (end !== -1) {
        flush();
        out.push({ kind: 'italic', children: parseInline(src.slice(i + 1, end)) });
        i = end + 1;
        continue;
      }
    }

    buf += ch;
    i += 1;
  }
  flush();
  return out;
}

/** 是否是表格的分隔行（`| --- | --- |`）。 */
function isDelimiterRow(line: string): boolean {
  return /^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)+\|?\s*$/.test(line);
}

/** 把 `| a | b |` 切成 `['a', 'b']`。 */
function splitRow(line: string): string[] {
  let cells = line.split('|');
  if (cells[0]?.trim() === '') cells = cells.slice(1);
  if (cells[cells.length - 1]?.trim() === '') cells = cells.slice(0, -1);
  return cells.map((c) => c.trim());
}

function parseAlign(line: string): (null | 'left' | 'center' | 'right')[] {
  return splitRow(line).map((cell) => {
    const left = cell.startsWith(':');
    const right = cell.endsWith(':');
    if (left && right) return 'center';
    if (right) return 'right';
    if (left) return 'left';
    return null;
  });
}

/** 是否是某个块的起始行（段落收集时用来判断要不要停下）。 */
function isBlockStart(line: string): boolean {
  return (
    /^```/.test(line) ||
    /^#{1,6}\s/.test(line) ||
    /^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/.test(line) ||
    /^\s*>\s?/.test(line) ||
    /^\s*[-*+]\s/.test(line) ||
    /^\s*\d+[.)]\s/.test(line)
  );
}

function parseBlocks(lines: string[]): MdBlock[] {
  const blocks: MdBlock[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i] ?? '';

    // 围栏代码块
    const fence = FENCE.exec(line);
    if (fence) {
      const lang = fence[1] ?? '';
      const buf: string[] = [];
      i += 1;
      while (i < lines.length && !/^```\s*$/.test(lines[i] ?? '')) {
        buf.push(lines[i] ?? '');
        i += 1;
      }
      i += 1; // 吃掉收尾围栏
      blocks.push({ kind: 'code', lang, content: buf.join('\n') });
      continue;
    }

    if (line.trim() === '') {
      i += 1;
      continue;
    }

    // 标题
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      blocks.push({
        kind: 'heading',
        level: heading[1]?.length ?? 1,
        content: parseInline(heading[2] ?? ''),
      });
      i += 1;
      continue;
    }

    // 分隔线
    if (/^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      blocks.push({ kind: 'hr' });
      i += 1;
      continue;
    }

    // 引用
    if (/^\s*>\s?/.test(line)) {
      const buf: string[] = [];
      while (i < lines.length && /^\s*>\s?/.test(lines[i] ?? '')) {
        buf.push((lines[i] ?? '').replace(/^\s*>\s?/, ''));
        i += 1;
      }
      blocks.push({ kind: 'quote', content: parseInline(buf.join(' ')) });
      continue;
    }

    // 表格：当前行 + 分隔行
    if (line.includes('|') && i + 1 < lines.length && isDelimiterRow(lines[i + 1] ?? '')) {
      const headers = splitRow(line).map(parseInline);
      const align = parseAlign(lines[i + 1] ?? '');
      i += 2;
      const rows: Inline[][][] = [];
      while (i < lines.length && (lines[i] ?? '').includes('|') && (lines[i] ?? '').trim() !== '') {
        rows.push(splitRow(lines[i] ?? '').map(parseInline));
        i += 1;
      }
      blocks.push({ kind: 'table', headers, align, rows });
      continue;
    }

    // 列表
    const ul = /^\s*[-*+]\s+(.*)$/.exec(line);
    const ol = /^\s*\d+[.)]\s+(.*)$/.exec(line);
    if (ul || ol) {
      const ordered = ol !== null;
      const items: Inline[][] = [];
      while (i < lines.length) {
        const marker = ordered
          ? /^\s*\d+[.)]\s+(.*)$/.exec(lines[i] ?? '')
          : /^\s*[-*+]\s+(.*)$/.exec(lines[i] ?? '');
        if (!marker) break;
        const parts = [marker[1] ?? ''];
        i += 1;
        // 续行：非空、不是新块起点、不是下一个列表项
        while (
          i < lines.length &&
          (lines[i] ?? '').trim() !== '' &&
          !isBlockStart(lines[i] ?? '') &&
          !(ordered ? /^\s*\d+[.)]\s/.test(lines[i] ?? '') : /^\s*[-*+]\s/.test(lines[i] ?? ''))
        ) {
          parts.push((lines[i] ?? '').trim());
          i += 1;
        }
        items.push(parseInline(parts.join(' ')));
      }
      blocks.push({ kind: 'list', ordered, items });
      continue;
    }

    // 段落
    const buf = [line.trim()];
    i += 1;
    while (i < lines.length && (lines[i] ?? '').trim() !== '' && !isBlockStart(lines[i] ?? '')) {
      buf.push((lines[i] ?? '').trim());
      i += 1;
    }
    blocks.push({ kind: 'paragraph', content: parseInline(buf.join(' ')) });
  }

  return blocks;
}

function renderInline(nodes: Inline[]): ReactNode[] {
  return nodes.map((node, i) => {
    switch (node.kind) {
      case 'text':
        return <span key={i}>{node.text}</span>;
      case 'code':
        return (
          <code
            key={i}
            className="rounded bg-(--bg-inset) px-1.5 py-0.5 font-mono text-[0.85em] text-(--fg)"
          >
            {node.text}
          </code>
        );
      case 'bold':
        return (
          <strong key={i} className="font-semibold text-(--fg)">
            {renderInline(node.children)}
          </strong>
        );
      case 'italic':
        return <em key={i}>{renderInline(node.children)}</em>;
      case 'strike':
        return (
          <s key={i} className="text-(--fg-muted)">
            {renderInline(node.children)}
          </s>
        );
      case 'link':
        return (
          <a
            key={i}
            href={node.href}
            target="_blank"
            rel="noreferrer"
            className="text-accent underline decoration-(--color-accent)/40 underline-offset-2 hover:decoration-(--color-accent)"
          >
            {renderInline(node.children)}
          </a>
        );
    }
  });
}

function Markdown({ text }: { text: string }) {
  const blocks = parseBlocks(text.split('\n'));

  return (
    <div className="selectable space-y-3">
      {blocks.map((block, i) => {
        switch (block.kind) {
          case 'code':
            return <CodeBlock key={i} lang={block.lang} content={block.content} />;
          case 'heading':
            return (
              <Heading key={i} level={block.level}>
                {renderInline(block.content)}
              </Heading>
            );
          case 'paragraph':
            return (
              <p key={i} className="leading-relaxed wrap-break-word">
                {renderInline(block.content)}
              </p>
            );
          case 'quote':
            return (
              <blockquote
                key={i}
                className="border-l-2 border-(--border-strong) pl-3 text-(--fg-muted)"
              >
                {renderInline(block.content)}
              </blockquote>
            );
          case 'list': {
            const Tag = block.ordered ? 'ol' : 'ul';
            return (
              <Tag key={i} className="space-y-1 pl-5">
                {block.items.map((item, j) => (
                  <li
                    key={j}
                    className={
                      block.ordered
                        ? 'list-decimal marker:text-(--fg-subtle)'
                        : 'list-disc marker:text-(--fg-subtle)'
                    }
                  >
                    {renderInline(item)}
                  </li>
                ))}
              </Tag>
            );
          }
          case 'table':
            return (
              <div key={i} className="overflow-x-auto rounded-card border border-(--border)">
                <table className="w-full border-collapse text-[13.5px]">
                  <thead>
                    <tr>
                      {block.headers.map((cell, j) => (
                        <th
                          key={j}
                          style={tableAlign(block.align[j] ?? null)}
                          className="border-b border-(--border) bg-(--bg-inset) px-3 py-2 text-left font-medium"
                        >
                          {renderInline(cell)}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {block.rows.map((row, r) => (
                      <tr key={r} className="odd:bg-(--bg-elevated)">
                        {block.headers.map((_, c) => (
                          <td
                            key={c}
                            style={tableAlign(block.align[c] ?? null)}
                            className="border-b border-(--border) px-3 py-2 align-top text-(--fg-muted)"
                          >
                            {renderInline(row[c] ?? [{ kind: 'text', text: '' }])}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            );
          case 'hr':
            return <hr key={i} className="border-(--border)" />;
        }
      })}
    </div>
  );
}

function tableAlign(align: null | 'left' | 'center' | 'right'): {
  textAlign?: 'left' | 'center' | 'right';
} {
  return align ? { textAlign: align } : {};
}

function Heading({ level, children }: { level: number; children: ReactNode }) {
  const size =
    level === 1
      ? 'text-xl font-semibold'
      : level === 2
        ? 'text-lg font-semibold'
        : level === 3
          ? 'text-base font-semibold'
          : 'text-[15px] font-semibold';
  return <div className={`${size} leading-relaxed text-(--fg)`}>{children}</div>;
}
