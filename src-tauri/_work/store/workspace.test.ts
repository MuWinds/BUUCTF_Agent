import { describe, expect, it } from 'vitest';
import { basename } from './workspace';

describe('basename', () => {
  it('同时认 Windows 和 POSIX 分隔符', () => {
    expect(basename('C:\\Users\\me\\proj')).toBe('proj');
    expect(basename('/home/me/proj')).toBe('proj');
  });

  it('忽略结尾的分隔符', () => {
    expect(basename('C:\\Users\\me\\proj\\')).toBe('proj');
    expect(basename('/home/me/proj//')).toBe('proj');
  });

  it('无分隔符或取不出末段时返回原串', () => {
    expect(basename('proj')).toBe('proj');
    expect(basename('/')).toBe('/');
  });
});
