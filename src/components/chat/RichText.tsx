import { memo } from 'react';

/**
 * 轻量文本渲染：只识别围栏代码块，其余按纯文本保留换行。
 *
 * 刻意不引 Markdown 解析库 —— 流式期间跑完整 Markdown + 语法高亮是同类应用
 * 掉帧的头号原因。完整 Markdown 留到后续阶段，且只在轮次结束后对定稿文本做一次。
 */

interface Block {
  kind: 'text' | 'code';
  lang?: string;
  content: string;
}

const FENCE = /^```([\w+-]*)\s*$/;

function parse(source: string): Block[] {
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
      // 进入代码块时记住语言；退出时清掉
      inCode = !inCode;
      lang = inCode ? (fence[1] ?? '') : '';
      continue;
    }
    buffer.push(line);
  }
  flush();

  return blocks;
}

export const RichText = memo(function RichText({ text }: { text: string }) {
  if (!text) return null;
  const blocks = parse(text);

  return (
    <div className="selectable space-y-3">
      {blocks.map((block, i) =>
        block.kind === 'code' ? (
          <CodeBlock key={i} lang={block.lang} content={block.content} />
        ) : (
          <p key={i} className="whitespace-pre-wrap break-words leading-relaxed">
            {block.content}
          </p>
        ),
      )}
    </div>
  );
});

function CodeBlock({ lang, content }: { lang?: string; content: string }) {
  return (
    <div className="overflow-hidden rounded-(--radius-card) border border-(--border) bg-(--bg-inset)">
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
