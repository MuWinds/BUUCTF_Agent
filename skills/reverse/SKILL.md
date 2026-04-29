---
name: reverse
description: 逆向工程技术，包括 ELF/PE 二进制分析、脱壳、反混淆、Android 逆向、固件分析等。
tags:
  - elf
  - pe
  - android
  - unpacking
  - deobfuscation
  - firmware
---

# 逆向工程方法论

## 通用分析流程

1. **文件识别**：`file` 命令确定文件类型、架构、保护机制
2. **字符串搜索**：`strings` 查找可疑字符串（flag、密码、提示信息）
3. **静态分析**：反汇编/反编译理解程序逻辑
4. **动态调试**：GDB/LLDB 跟踪执行流程，下断点分析关键函数

## ELF 分析

- **基本信息**：`file`、`readelf -h`、`checksec`
- **符号表**：`nm`、`readelf -s` 查看导出符号
- **反编译**：IDA Pro、Ghidra、Binary Ninja
- **调试**：GDB + pwndbg/peda/gef 插件
- **保护机制**：NX、ASLR、PIE、Stack Canary、RELRO

## PE 分析

- **基本信息**：`file`、PEiD 查壳、Detect It Easy
- **脱壳**：ESP 定律、单步跟踪、内存 dump + 修复导入表
- **.NET**：dnSpy 反编译、de4dot 去混淆
- **Delphi**：IDA 识别 VCL 结构、事件处理函数

## 常见算法识别

- **Base64**：查找字符表 `ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/`
- **MD5/SHA**：查找初始化常量（MD5: `0x67452301`，SHA1: `0x67452301`）
- **AES**：查找 S-Box（`0x63, 0x7c, 0x77, 0x7b...`）或轮常量
- **RC4**：查找 256 字节初始化循环
- **TEA/XTEA**：查找常量 `0x9e3779b9`（黄金比例）
- **SM4**：查找 S-Box 或 CK 常量

## 反调试绕过

- **ptrace 检测**：`LD_PRELOAD` 注入或 patch `ptrace` 调用
- **时间检测**：patch `rdtsc` 或 `time` 相关调用
- **/proc 检测**：`open` syscall hook 或修改检测逻辑
- **信号**：自定义信号处理器覆盖默认行为

## Android 逆向

- **APK 解包**：apktool、jadx、jeb
- **Java 层**：jadx 反编译 DEX -> Java
- **Native 层**：IDA 分析 `.so` 文件
- **Hook**：Frida 动态 hook、Xposed 框架
- **脱壳**：Frida dump + 修复、BlackDex

## 固件分析

- **提取**：binwalk `-e` 提取文件系统
- **模拟**：QEMU user-mode 或 system-mode
- **调试**：gdbserver 附加到模拟进程
- **文件系统**：squashfs、jffs2、cramfs 等

## 常用工具速查

```bash
file <binary>            # 文件类型识别
strings <binary>         # 字符串提取
readelf -a <binary>      # ELF 头信息
objdump -d <binary>      # 反汇编
checksec --file=<binary> # 安全保护检查
strace ./binary          # 系统调用跟踪
ltrace ./binary          # 库函数调用跟踪
```
