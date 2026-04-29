---
name: forensics-disk
description: 磁盘取证分析技术，涵盖 NTFS/FAT/ext 文件系统解析、文件恢复、时间线分析、日志挖掘等实战技能。
tags:
  - ntfs
  - fat
  - ext4
  - file-recovery
  - timeline
  - windows-event
---

# 磁盘取证分析方法论

## 整体分析流程

1. **镜像挂载**：使用 `mount -o loop,ro` 只读挂载镜像，或用 FTK Imager / Autopsy 加载
2. **文件系统识别**：`fsstat` 或 `file -s` 确认文件系统类型和参数
3. **目录浏览**：`fls -r` 递归列出所有文件（含已删除），`istat` 查看 inode/MFT 详情
4. **关键文件提取**：`icat` 按 inode/MFT 编号提取文件内容
5. **时间线构建**：`mactime` 从 MAC 时间戳生成完整活动时间线

## NTFS 文件系统

### MFT (Master File Table)

- 每个文件/目录对应一个 MFT 记录，编号从 0 开始
- $MFT (0)、$MFTMirr (1)、$LogFile (2)、$Volume (3)、$AttrDef (4)、$Root (5)、$Bitmap (6)、$Boot (7)
- 常用命令：
  ```bash
  istat -f ntfs image.dd <MFT编号>   # 查看 MFT 记录详情
  icat -f ntfs image.dd <MFT编号>    # 提取文件内容
  fls -f ntfs -d image.dd            # 仅列出已删除文件
  ```

### $LogFile 与 $UsnJrnl

- `$LogFile`：NTFS 事务日志，记录元数据变更，可恢复近期修改
- `$UsnJrnl`：USN 变更日志，记录文件创建/删除/重命名等操作
- 提取工具：`MFTECmd`、`NTFS Log Tracker`

### Alternate Data Streams (ADS)

- NTFS 支持在文件上附加多个数据流，常用于隐藏数据
  ```bash
  # 查看 ADS
  streams <file>
  # Sleuth Kit 方式
  fls -r -f ntfs image.dd | grep ":"
  ```

### 时间戳 (MACB)

- **M** (Modified)：文件内容最后修改时间
- **A** (Accessed)：文件最后访问时间
- **C** (Created/MFT Changed)：MFT 记录最后变更时间
- **B** (Born)：文件创建时间（仅 $STANDARD_INFORMATION 有）
- `$FILE_NAME` 和 `$STANDARD_INFORMATION` 可能有不同的时间戳，注意对比

## FAT 文件系统

### 关键结构

- **FAT 表**：记录簇链，`0x0FFFFFFF` 标记文件结束
- **目录项**：32 字节，含文件名、起始簇、大小、时间
- **长文件名 (LFN)**：连续多个 32 字节目录项存储 Unicode 文件名

### 已删除文件恢复

- 删除时首字节改为 `0xE5`，簇链清零，但数据区未擦除
- 恢复方法：
  ```bash
  fls -f fat -d image.dd           # 列出已删除文件
  icat -f fat image.dd <簇号>      # 提取内容
  # 或使用 testdisk / photorec
  ```

## ext4 文件系统

### 关键概念

- **inode**：存储文件元数据（权限、时间、块指针）
- **块组**：文件系统划分为多个块组，每组有自己的超级块备份
- **日志 (Journal)**：JBD2 日志记录元数据操作，`journalctl` 或直接解析

### 已删除文件恢复

- ext4 默认开启 `dir_index`，删除后 inode 标记清零
- 使用 `extundelete` 或 `ext4magic` 恢复
  ```bash
  extundelete --restore-all /dev/sdX1
  ext4magic image.dd -f /path/to/deleted/file -d output/
  ```

### 时间戳分析

- `istat -f ext4 image.dd <inode号>` 查看 atime/mtime/ctime/ctime
- `debugfs` 进入交互式调试模式：
  ```bash
  debugfs image.dd
  debugfs: ls -l /path/to/dir
  debugfs: stat <inode>
  ```

## Windows 事件日志

### 常见日志位置

- 系统事件：`C:\Windows\System32\winevt\Logs\System.evtx`
- 安全事件：`C:\Windows\System32\winevt\Logs\Security.evtx`
- 应用事件：`C:\Windows\System32\winevt\Logs\Application.evtx`
- PowerShell 日志：`Microsoft-Windows-PowerShell%4Operational.evtx`
- RDP 登录：`Microsoft-Windows-TerminalServices-LocalSessionManager%4Operational.evtx`

### 关键事件 ID

| ID | 来源 | 含义 |
|----|------|------|
| 4624 | Security | 登录成功 |
| 4625 | Security | 登录失败 |
| 4634 | Security | 注销 |
| 4688 | Security | 新进程创建 |
| 4720 | Security | 账户创建 |
| 7045 | System | 服务安装 |
| 1102 | Security | 日志清除 |

### 解析工具

```bash
# Python 解析
python3 -c "
import Evtx.Evtx as evtx
with evtx.Evtx('Security.evtx') as log:
    for record in log.records():
        print(record.xml())
"
# 命令行工具
wevtx_dump Security.evtx
chainsaw hunt Security.evtx --mapping sigma
```

## 浏览器痕迹

### Chrome/Edge (Chromium)

- 历史记录：`%LOCALAPPDATA%\Google\Chrome\User Data\Default\History`（SQLite）
- 下载记录：同上文件的 `downloads` 表
- Cookie：`%LOCALAPPDATA%\Google\Chrome\User Data\Default\Cookies`
- 缓存：`%LOCALAPPDATA%\Google\Chrome\User Data\Default\Cache\`

### Firefox

- 历史记录：`%APPDATA%\Mozilla\Firefox\Profiles\<profile>\places.sqlite`
- 下载记录：`downloads.sqlite`（旧版）或 `places.sqlite`
- Cookie：`cookies.sqlite`

### 查询示例

```sql
-- Chrome 历史记录
SELECT datetime(last_visit_time/1000000-11644473600,'unixepoch','localtime'),
       url, title, visit_count
FROM urls ORDER BY last_visit_time DESC;

-- Chrome 下载记录
SELECT datetime(start_time/1000000-11644473600,'unixepoch','localtime'),
       target_path, total_bytes, state
FROM downloads ORDER BY start_time DESC;
```

## 常用工具速查

```bash
# Sleuth Kit 命令行
fls -r -m / image.dd              # 列出文件（TSK 路径格式）
mactime -b body.txt -d            # 生成时间线
tsk_recover image.dd output/      # 批量恢复已删除文件
blkstat -f ntfs image.dd <簇号>   # 查看块分配状态
mmstat image.dd                   # 查看分区表

# Autopsy / FTK
# GUI 工具，适合综合分析

# 系统相关
reglookup NTUSER.DAT              # 注册表解析
regripper -r NTUSER.DAT -f ntuser # 注册表信息提取
```
