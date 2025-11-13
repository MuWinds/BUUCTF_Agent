"""FastAPI server powering the BUUCTF Agent WebUI."""
from __future__ import annotations

import asyncio
from pathlib import Path
from typing import Any, Dict, List

from fastapi import (
    FastAPI,
    File,
    HTTPException,
    UploadFile,
    WebSocket,
    WebSocketDisconnect,
)
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel

from .config_manager import load_config, save_config, active_config_path
from .event_bus import EventBus
from .session import AgentSession

BASE_DIR = Path(__file__).resolve().parent
STATIC_DIR = BASE_DIR / "static"
INDEX_FILE = STATIC_DIR / "index.html"
ATTACHMENTS_DIR = Path("attachments")
ATTACHMENTS_DIR.mkdir(parents=True, exist_ok=True)

app = FastAPI(title="BUUCTF Agent WebUI", version="1.0.0")
app.mount("/static", StaticFiles(directory=STATIC_DIR), name="static")

_event_bus = EventBus()
_session = AgentSession(event_bus=_event_bus)


def _list_attachments() -> List[Dict[str, Any]]:
    attachments: List[Dict[str, Any]] = []
    if not ATTACHMENTS_DIR.exists():
        return attachments

    for file_path in ATTACHMENTS_DIR.iterdir():
        if file_path.is_file():
            try:
                size = file_path.stat().st_size
            except OSError:
                size = 0
            attachments.append({"name": file_path.name, "size": size})
    return sorted(attachments, key=lambda item: item["name"].lower())


class StartRequest(BaseModel):
    question: str


class FlagDecision(BaseModel):
    approve: bool


@app.get("/", response_class=FileResponse)
async def index() -> FileResponse:
    if not INDEX_FILE.exists():  # pragma: no cover - sanity guard
        raise HTTPException(status_code=500, detail="前端资源缺失")
    return FileResponse(str(INDEX_FILE))


@app.get("/api/status")
async def get_status() -> Dict[str, str]:
    return {"status": _session.get_status()}


@app.get("/api/config")
async def get_config() -> Dict[str, Any]:
    config = load_config()
    return {"config": config, "source": str(active_config_path())}


@app.put("/api/config")
async def update_config(config: Dict[str, Any]) -> Dict[str, str]:
    try:
        save_config(config)
    except Exception as exc:  # pragma: no cover - validation fallback
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    return {"status": "saved"}


@app.post("/api/start")
async def start_agent(payload: StartRequest) -> Dict[str, str]:
    try:
        _session.start(payload.question)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail=str(exc)) from exc
    except RuntimeError as exc:
        raise HTTPException(status_code=409, detail=str(exc)) from exc
    return {"status": "started"}


@app.post("/api/terminate")
async def terminate_agent() -> Dict[str, str]:
    _session.terminate()
    return {"status": _session.get_status()}


@app.post("/api/flag")
async def submit_flag_decision(decision: FlagDecision) -> Dict[str, bool]:
    accepted = _session.decide_flag(decision.approve)
    if not accepted:
        raise HTTPException(status_code=409, detail="当前没有待确认的 flag")
    return {"accepted": True}


@app.get("/api/attachments")
async def get_attachments() -> Dict[str, Any]:
    return {"attachments": _list_attachments()}


@app.post("/api/attachments")
async def upload_attachments(
    files: List[UploadFile] = File(..., description="待上传的附件文件"),
) -> Dict[str, Any]:
    if not files:
        raise HTTPException(status_code=400, detail="请选择要上传的文件")

    ATTACHMENTS_DIR.mkdir(parents=True, exist_ok=True)
    saved: List[str] = []

    for upload in files:
        filename = Path(upload.filename or "").name
        if not filename:
            await upload.close()
            continue

        destination = ATTACHMENTS_DIR / filename
        try:
            data = await upload.read()
            destination.write_bytes(data)
            saved.append(filename)
        finally:
            await upload.close()

    if not saved:
        raise HTTPException(status_code=400, detail="没有成功写入的附件")

    return {"saved": saved, "attachments": _list_attachments()}


@app.delete("/api/attachments/{filename}")
async def delete_attachment(filename: str) -> Dict[str, Any]:
    safe_name = Path(filename).name
    if safe_name != filename:
        raise HTTPException(status_code=400, detail="非法文件名")

    file_path = ATTACHMENTS_DIR / safe_name
    if not file_path.exists() or not file_path.is_file():
        raise HTTPException(status_code=404, detail="文件不存在")

    file_path.unlink()
    return {"deleted": safe_name, "attachments": _list_attachments()}


@app.delete("/api/attachments")
async def clear_attachments() -> Dict[str, Any]:
    deleted = 0
    if ATTACHMENTS_DIR.exists():
        for file_path in ATTACHMENTS_DIR.iterdir():
            if file_path.is_file():
                file_path.unlink()
                deleted += 1

    return {"deleted": deleted, "attachments": _list_attachments()}


@app.websocket("/ws/events")
async def events(websocket: WebSocket) -> None:
    await websocket.accept()
    subscriber_queue = _event_bus.subscribe()

    # Immediately push the current status to the new client.
    await websocket.send_json({"type": "status", "status": _session.get_status()})

    try:
        loop = asyncio.get_running_loop()
        while True:
            event = await loop.run_in_executor(None, subscriber_queue.get)
            await websocket.send_json(event)
    except WebSocketDisconnect:
        pass
    finally:
        _event_bus.unsubscribe(subscriber_queue)
