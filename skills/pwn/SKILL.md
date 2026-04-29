---
name: pwn
description: 二进制漏洞利用技术，包括栈溢出、堆利用、格式化字符串、ROP 链构造、内核漏洞利用等。
tags:
  - stack-overflow
  - heap-exploit
  - format-string
  - rop
  - kernel
  - ret2libc
---

# PWN 漏洞利用方法论

## 通用流程

1. **分析二进制**：`checksec` 查看保护，`file` 确认架构
2. **识别漏洞**：逆向分析找到溢出点、UAF、格式化字符串等
3. **确定利用策略**：根据保护机制选择利用方式
4. **编写 Exploit**：使用 pwntools 编写攻击脚本
5. **调试验证**：本地调试确认 exploit 正确性

## 栈溢出

### 基础栈溢出
- **无保护**：直接覆盖返回地址跳转到 `system("/bin/sh")`
- **ret2libc**：泄露 libc 地址 -> 计算 system/`/bin/sh` 地址 -> 调用
- **ret2text**：利用程序中已有的 `system` 调用
- **ret2shellcode**：在可执行内存区域注入 shellcode

### 绕过技巧
- **NX 绕过**：ROP / ret2libc / mprotect 修改权限
- **ASLR 绕过**：泄露地址 -> 计算偏移 -> 重用
- **Canary 绕过**：逐字节爆破 / 泄露 canary 值
- **PIE 绕过**：泄露代码基址 -> 计算偏移

## 堆利用

### glibc 堆管理基础
- **chunk 结构**：`prev_size | size | fd | bk | ...`
- **bins**：fastbin、unsortedbin、smallbin、largebin
- **tcache**：glibc 2.26+ 的 per-thread 缓存

### 常见技术
- **UAF (Use-After-Free)**：释放后未清空指针，重新分配后操控
- **Double Free**：同一 chunk 释放两次，fastbin 链表操控
- **House of Spirit**：伪造 chunk 实现任意地址分配
- **House of Force**：通过 top chunk 大小溢出实现任意地址分配
- **House of Lore**：smallbin 链表操控
- **House of Orange**：修改 top chunk 触发 sysmalloc -> unsortedbin attack
- **Unsortedbin Attack**：利用 unsorted bin 的 bk 指针写入 main_arena 地址
- **Largebin Attack**：利用 largebin 的 fd_nextsize/bk_nextsize

### glibc 版本差异
- **2.23**：无 tcache，fastbin 不检查 double free
- **2.26-2.28**：引入 tcache，tcache 不检查 double free
- **2.29+**：tcache 增加 key 字段检测 double free
- **2.31+**：增加 safe-linking（地址混淆）

## 格式化字符串

- **读取栈数据**：`%p`、`%x`、`%lx` 泄露栈内容
- **任意地址读**：`%N$s` + 构造栈上的地址指针
- **任意地址写**：`%N$n`（4字节）、`%N$hn`（2字节）、`%N$hhn`（1字节）
- **GOT 覆写**：修改 GOT 表中函数地址
- **偏移计算**：找到输入在栈上的位置（`AAAA%p.%p.%p...`）

## ROP 技术

- **基础 ROP**：`pop rdi; ret` + `system@plt` + `"/bin/sh"`
- **ret2csu**：利用 `__libc_csu_init` 中的 gadget
- **SROP**：利用 `sigreturn` 系统调用设置所有寄存器
- **ret2dlresolve**：劫持动态链接过程
- **工具**：ROPgadget、ropper、one_gadget

## IO 利用

- **FILE 结构体**：伪造 `_IO_FILE` 结构体
- **FSOP (File Stream Oriented Programming)**：利用 `_IO_list_all` 链表
- **House of Pig**：tcache attack + FSOP
- **House of Banana**：利用 `_rtld_global` 劫持控制流

## 常用工具

```python
from pwn import *
# pwntools 核心用法
p = process("./binary")    # 本地
p = remote("host", port)   # 远程
elf = ELF("./binary")      # 加载二进制
libc = ELF("./libc.so")    # 加载 libc
p.recvuntil(b": ")         # 接收直到
p.sendline(payload)        # 发送
p.interactive()            # 交互模式
```

```bash
one_gadget libc.so.6       # 查找 one_gadget
ROPgadget --binary binary  # 查找 ROP gadget
ropper -f binary           # 另一个 gadget 工具
```
