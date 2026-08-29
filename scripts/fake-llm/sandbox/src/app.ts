import { formatBytes } from './util';

const GREETING = 'hello world';

export function main(): void {
  console.log(GREETING);
  console.log(formatBytes(1024));
}
