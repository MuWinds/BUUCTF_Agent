import { describe, expect, it } from 'vitest';
import { errorMessage } from './events';

describe('errorMessage', () => {
  it('原样返回字符串错误', () => {
    expect(errorMessage('连接超时')).toBe('连接超时');
  });

  it('取出 AppError 的 message', () => {
    expect(errorMessage({ code: 'llm', message: '模型名为空', retryable: false })).toBe(
      '模型名为空',
    );
  });

  it('对没有 message 的值退化成 String()', () => {
    expect(errorMessage(404)).toBe('404');
    expect(errorMessage(null)).toBe('null');
  });
});
