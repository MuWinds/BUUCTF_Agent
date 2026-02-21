"""CTF Agent 工具层 - http_request"""
import hashlib
import time
from typing import Optional

import httpx
from langchain_core.tools import tool


@tool
def http_request(
    url: str,
    method: str = "GET",
    headers: Optional[dict] = None,
    params: Optional[dict] = None,
    data: Optional[str] = None,
    json_body: Optional[dict] = None,
    timeout_sec: int = 10,
    allow_redirects: bool = True,
) -> dict:
    """发送 HTTP 请求到目标 URL，用于 Web 侦察、获取页面内容、提交表单等。

    Args:
        url: 目标 URL（必填）
        method: HTTP 方法，GET/POST/PUT/DELETE/HEAD
        headers: 可选的请求头
        params: 可选的 query 参数
        data: 可选的表单/原始 body
        json_body: 可选的 JSON body
        timeout_sec: 超时秒数，默认 10
        allow_redirects: 是否跟随重定向，默认 true
    """
    start = time.time()
    try:
        with httpx.Client(follow_redirects=allow_redirects, timeout=timeout_sec) as client:
            resp = client.request(
                method=method.upper(),
                url=url,
                headers=headers,
                params=params,
                content=data.encode("utf-8") if data else None,
                json=json_body,
            )

        elapsed_ms = round((time.time() - start) * 1000)
        body_bytes = resp.content
        text_preview = resp.text[:2000] if resp.text else ""

        return {
            "status_code": resp.status_code,
            "headers": dict(resp.headers),
            "text_preview": text_preview,
            "bytes_len": len(body_bytes),
            "elapsed_ms": elapsed_ms,
            "sha256": hashlib.sha256(body_bytes).hexdigest(),
        }
    except Exception as e:
        return {
            "error": str(e),
            "elapsed_ms": round((time.time() - start) * 1000),
        }
