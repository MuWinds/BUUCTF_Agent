import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const SRC_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SELF = fileURLToPath(import.meta.url);

/** 递归收集 src 下的 TS/TSX 源码，排除本文件 —— 它自身含有反面样例。 */
function sourceFiles(): string[] {
  return readdirSync(SRC_DIR, { recursive: true, encoding: 'utf8' })
    .map((entry) => path.join(SRC_DIR, entry))
    .filter((file) => /\.tsx?$/.test(file) && file !== SELF);
}

/** 逐行搜索并返回 `文件:行号  命中内容` 形式的清单，断言失败时能直接定位。 */
function findAll(pattern: RegExp): string[] {
  const hits: string[] = [];
  for (const file of sourceFiles()) {
    const relative = path.relative(SRC_DIR, file).replace(/\\/g, '/');
    readFileSync(file, 'utf8')
      .split('\n')
      .forEach((line, index) => {
        for (const match of line.matchAll(pattern)) {
          hits.push(`${relative}:${index + 1}  ${match[0]}`);
        }
      });
  }
  return hits;
}

describe('Tailwind 类名写法', () => {
  // Tailwind v4 给 CSS 变量提供了 x-(--var) 简写。长形式 x-[var(--var)] 编译出的
  // 声明完全相同，但工具链会对每一处报「can be written as」，几十条噪音会盖住真问题。
  it('CSS 变量用 v4 简写 x-(--var)，不写成 x-[var(--var)]', () => {
    expect(findAll(/[\w:-]+-\[var\(--[\w-]+\)\]/g)).toEqual([]);
  });
});
