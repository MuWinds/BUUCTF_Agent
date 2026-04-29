---
name: crypto
description: 密码学攻击技术，包括古典密码、RSA、AES、哈希碰撞、椭圆曲线等密码系统的分析与破解。
tags:
  - classical-cipher
  - rsa
  - aes
  - hash
  - ecc
  - encoding
---

# 密码学攻击方法论

## 编码识别

- **Base64**：字符集 `A-Za-z0-9+/`，末尾 `=` 填充
- **Base32**：字符集 `A-Z2-7`，末尾 `=` 填充
- **Hex**：`0-9a-f` 或 `0-9A-F`
- **URL 编码**：`%xx` 格式
- **Unicode**：`\uXXXX` 或 `&#xXXXX;`
- **Brainfuck/Ook!**：特殊字符组成的 esolang

## 古典密码

- **凯撒/移位**：枚举 26 种偏移，或频率分析
- **维吉尼亚**：Kasiski 测试确定密钥长度，频率分析破解
- **栅栏密码**：尝试不同栏数，W型需先还原路径
- **Playfair**：5x5 矩阵，双字母组加密
- **培根密码**：5 位二进制编码，注意 A/B 的对应关系

## RSA

- **小公钥指数**：`e=3` 时直接开立方根
- **共模攻击**：同一 n 不同 e 加密同一消息，`gcd(c1^e2, c2^e1)` 扩展欧几里得
- **因数分解**：小因子直接分解，yafu/msieve 工具
- **Wiener 攻击**：`d < n^(1/4)` 时连分数展开
- **p-1 光滑**：Pollard's p-1 算法
- **Coppersmith**：已知明文高位，Sage `small_roots()`
- **多素数 RSA**：`n = p*q*r`，`phi = (p-1)(q-1)(r-1)`
- **常见工具**：RsaCtfTool、yafu、SageMath

## AES

- **ECB 模式**：相同明文块产生相同密文，可进行模式分析
- **CBC 字节翻转**：修改前一密文块的第 i 字节影响下一明文块第 i 字节
- **Padding Oracle**：利用填充错误信息解密，`padbuster` 工具
- **密钥恢复**：侧信道攻击、弱密钥

## 哈希

- **长度扩展攻击**：MD5/SHA1 的 Merkle-Damgard 结构，`hashpump` 工具
- **碰撞**：MD5 的 `fastcoll` 工具
- **彩虹表**：在线查询 crackstation.net、cmd5.com

## 椭圆曲线 (ECC)

- **小阶攻击**：Pohlig-Hellman 算法
- **Smart 攻击**：`#E(F_p) = p` 时
- **MOV 攻击**：嵌入度较小时
- **Sage 工具**：`discrete_log()`、`lift_x()`

## 常见解题工具

```bash
# 在线工具
# CyberChef: 编解码、加解密瑞士军刀
# dcode.fr: 古典密码识别与破解
# factordb.com: 大数分解数据库

# 命令行工具
hashcat          # 哈希破解
john             # John the Ripper
openssl          # 加解密操作
python3 -c "import sympy; sympy.factorint(n)"  # 大数分解
```
