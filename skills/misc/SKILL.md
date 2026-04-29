---
name: misc
description: 杂项技术，包括隐写术、流量分析、编码转换、取证分析、AI 安全等非传统 CTF 分类。
tags:
  - steganography
  - forensics
  - traffic-analysis
  - encoding
  - osint
---

# 杂项 (Misc) 方法论

## 隐写术 (Steganography)

### 图片隐写
- **基础检查**：`file`、`exiftool` 查看元数据、`strings` 搜索隐藏文本
- **LSB 隐写**：`stegsolve` 查看各 bit plane、`zsteg` 自动检测
- **文件拼接**：`binwalk -e` 提取嵌入文件、`foremost` 文件恢复
- **GIF 隐写**：逐帧分析，`identify` 分解帧
- **PNG**：检查 IDAT 块、CRC 校验、IEND 之后的数据
- **JPG**：检查 EXIF 注释、APP 段、EOI 之后的数据
- **盲水印**：频域分析、两张图片异或

### 音频隐写
- **频谱图**：`Audacity` 或 `sox` 查看频谱，可能隐藏图像
- **LSB**：音频 LSB 嵌入数据
- **DTMF**：电话拨号音编码
- **SSTV**：慢扫描电视信号，`RX-SSTV` 解码

### 文本隐写
- **零宽字符**：Unicode 零宽空格/连接符隐藏信息
- **Snow**：空白字符（空格、Tab）编码
- **Unicode 同形字**：视觉相同但编码不同的字符

## 流量分析

### PCAP 分析流程
1. **协议统计**：`Statistics -> Protocol Hierarchy`
2. **会话分析**：`Statistics -> Conversations`
3. **字符串搜索**：`Strings (Ctrl+F)` 搜索 flag、password 等关键字
4. **导出对象**：`File -> Export Objects -> HTTP/DNS/SMB`

### 常见协议
- **HTTP**：追踪流 (Follow TCP Stream)，检查请求/响应体
- **DNS**：子域名编码数据（DNS tunneling）
- **FTP**：`ftp-data` 传输的文件内容
- **SMTP/POP3**：邮件内容和附件
- **USB**：键盘流量还原击键（HID usage ID 表）
- **蓝牙**：L2CAP 数据重组

### 工具
```bash
tshark -r capture.pcap          # 命令行 Wireshark
tshark -r cap.pcap -Y "http"    # 过滤 HTTP
tcpflow -r capture.pcap         # TCP 流重组
NetworkMiner                    # 网络取证工具
```

## 取证分析

### 磁盘取证
- **Autopsy/Sleuth Kit**：磁盘镜像分析
- **文件恢复**：`photorec`、`testdisk`
- **时间线分析**：`fls`、`mactime`

### 内存取证
- **Volatility**：内存镜像分析框架
- **常用命令**：
  - `imageinfo`：识别操作系统
  - `pslist`/`psscan`：进程列表
  - `filescan`/`dumpfiles`：文件恢复
  - `hashdump`：提取密码哈希
  - `clipboard`：剪贴板内容

### 系统日志
- **Windows**：事件日志 (`.evtx`)、注册表、Prefetch
- **Linux**：`/var/log/`、`.bash_history`、`/etc/passwd`

## 编码与转换

- **CyberChef**：在线编解码瑞士军刀
- **常见编码**：Base64/32/16、URL、HTML entity、Unicode
- **进制转换**：二进制、八进制、十进制、十六进制
- **摩尔斯电码**：`. -` 编码
- **猪圈密码**：图形替换密码
- **旗语**：手旗信号编码

## OSINT (开源情报)

- **图片搜索**：Google 以图搜图、TinEye
- **元数据**：`exiftool` 提取 GPS 坐标、设备信息
- **社工**：用户名搜索、社交媒体分析
- **Wayback Machine**：网页历史快照
