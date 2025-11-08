const statusIndicator = document.getElementById("statusIndicator");
const logStream = document.getElementById("logStream");
const questionInput = document.getElementById("questionInput");
const startButton = document.getElementById("startButton");
const stopButton = document.getElementById("stopButton");
const reloadConfigButton = document.getElementById("reloadConfig");
const saveConfigButton = document.getElementById("saveConfig");
const clearLogButton = document.getElementById("clearLog");
const configForm = document.getElementById("configForm");
const configSource = document.getElementById("configSource");
const attachmentInput = document.getElementById("attachmentInput");
const uploadAttachmentsButton = document.getElementById("uploadAttachments");
const clearAttachmentsButton = document.getElementById("clearAttachments");
const attachmentList = document.getElementById("attachmentList");
const attachmentsCount = document.getElementById("attachmentsCount");
const flagDialog = document.getElementById("flagDialog");
const flagValue = document.getElementById("flagValue");
const confirmFlagButton = document.getElementById("confirmFlag");
const rejectFlagButton = document.getElementById("rejectFlag");

let socket;
let reconnectTimer;
let awaitingFlag = false;
let currentConfig = {};
let currentAttachments = [];

const STATUS_MAP = {
    idle: "空闲",
    running: "运行中",
    awaiting_flag: "等待确认",
    terminating: "正在终止",
};

function setStatus(status) {
    const label = STATUS_MAP[status] || status;
    statusIndicator.textContent = label;
    statusIndicator.className = `status-chip ${status}`;

    const isBusy = status !== "idle";
    startButton.disabled = isBusy;
    stopButton.disabled = !isBusy;
}

function appendLog(title, message, type = "default") {
    const entry = document.createElement("div");
    entry.className = `log-entry ${type}`;

    const meta = document.createElement("div");
    meta.className = "meta";
    const label = document.createElement("strong");
    label.textContent = title;
    const timestamp = document.createElement("span");
    timestamp.textContent = new Date().toLocaleTimeString();
    meta.appendChild(label);
    meta.appendChild(timestamp);

    const body = document.createElement("div");
    body.className = "message";
    body.textContent = message;

    entry.appendChild(meta);
    entry.appendChild(body);
    if (logStream.firstChild) {
        logStream.insertBefore(entry, logStream.firstChild);
    } else {
        logStream.appendChild(entry);
    }
    logStream.scrollTop = 0;

    const maxEntries = 300;
    while (logStream.childElementCount > maxEntries) {
        logStream.removeChild(logStream.lastElementChild);
    }
}

function formatJSON(value) {
    try {
        return JSON.stringify(value, null, 2);
    } catch (err) {
        return String(value);
    }
}

function handleEvent(event) {
    if (!event || typeof event !== "object") {
        return;
    }

    switch (event.type) {
        case "status":
            setStatus(event.status);
            if (awaitingFlag && event.status !== "awaiting_flag") {
                awaitingFlag = false;
                flagDialog.classList.add("hidden");
                flagValue.textContent = "";
            }
            break;
        case "problem_received":
            appendLog("题面已接收", event.content || "", "info");
            break;
        case "problem_summary":
            appendLog("题面摘要", event.content || "", "info");
            break;
        case "analysis_complete":
            appendLog(
                "分析完成",
                formatJSON(event.analysis),
                "info"
            );
            break;
        case "solve_started":
            appendLog(
                "开始解题",
                `分类: ${event.category}\n思路: ${event.solution_plan}`,
                "success"
            );
            break;
        case "step_begin":
            appendLog("执行步骤", `第 ${event.step} 步`, "info");
            break;
        case "step_retry":
            appendLog("重新规划", `第 ${event.step} 步重试`, "warn");
            break;
        case "step_plan":
            appendLog(
                "工具计划",
                `步骤 ${event.step}\n工具: ${event.tool}\n参数:\n${formatJSON(
                    event.arguments
                )}`,
                "info"
            );
            break;
        case "tool_output":
            appendLog(
                `工具输出 (${event.tool})`,
                event.output || "(无输出)",
                "default"
            );
            break;
        case "step_analysis":
            appendLog(
                "结果分析",
                formatJSON(event.analysis),
                "info"
            );
            break;
        case "flag_candidate":
            awaitingFlag = true;
            flagValue.textContent = event.flag || "";
            flagDialog.classList.remove("hidden");
            appendLog(
                "发现 Flag", event.flag || "", "flag"
            );
            break;
        case "flag_rejected":
            appendLog("Flag 未确认", event.flag || "", "warn");
            break;
        case "run_error":
            appendLog("运行出错", event.message || "未知错误", "error");
            setStatus("idle");
            break;
        case "run_completed":
        case "solve_finished":
            appendLog("任务完成", event.result || "无结果", "success");
            break;
        case "terminated":
            appendLog(
                "流程结束",
                `原因: ${event.reason || "未知"}`,
                event.reason === "user_stop" ? "warn" : "error"
            );
            break;
        default:
            appendLog(
                `事件: ${event.type}`,
                formatJSON(event),
                "default"
            );
            break;
    }
}

function humanFileSize(size) {
    if (!Number.isFinite(size)) {
        return "-";
    }
    if (size < 1024) {
        return `${size} B`;
    }
    if (size < 1024 * 1024) {
        return `${(size / 1024).toFixed(1)} KB`;
    }
    return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}

function encodePath(path) {
    return path.join("::");
}

function decodePath(pathString) {
    return pathString ? pathString.split("::") : [];
}

function detectValueType(value) {
    if (Array.isArray(value)) {
        return "array";
    }
    if (value === null) {
        return "null";
    }
    const valueType = typeof value;
    if (["string", "number", "boolean"].includes(valueType)) {
        return valueType;
    }
    return "unknown";
}

function renderConfigForm() {
    if (!configForm) {
        return;
    }

    configForm.innerHTML = "";

    if (!currentConfig || typeof currentConfig !== "object") {
        const emptyMessage = document.createElement("div");
        emptyMessage.className = "config-hint";
        emptyMessage.textContent = "未读取到有效配置";
        configForm.appendChild(emptyMessage);
        return;
    }

    renderConfigSection(configForm, currentConfig, []);
}

function renderConfigSection(container, value, path) {
    Object.entries(value).forEach(([key, entryValue]) => {
        const fullPath = [...path, key];

        if (
            entryValue !== null &&
            typeof entryValue === "object" &&
            !Array.isArray(entryValue)
        ) {
            const details = document.createElement("details");
            details.open = path.length === 0;

            const summary = document.createElement("summary");
            summary.textContent = key;
            details.appendChild(summary);

            const section = document.createElement("div");
            section.className = "config-section";
            renderConfigSection(section, entryValue, fullPath);
            details.appendChild(section);

            container.appendChild(details);
        } else {
            container.appendChild(createConfigField(key, entryValue, fullPath));
        }
    });
}

function createConfigField(key, value, path) {
    const field = document.createElement("div");
    field.className = "config-field";

    const label = document.createElement("label");
    label.textContent = key;
    field.appendChild(label);

    const type = detectValueType(value);
    let input;

    if (type === "boolean") {
        input = document.createElement("select");
        [
            { value: "true", label: "true" },
            { value: "false", label: "false" },
        ].forEach((optionInfo) => {
            const option = document.createElement("option");
            option.value = optionInfo.value;
            option.textContent = optionInfo.label;
            if (String(value) === optionInfo.value) {
                option.selected = true;
            }
            input.appendChild(option);
        });
    } else if (type === "number") {
        input = document.createElement("input");
        input.type = "number";
        input.step = "any";
        input.value = value === undefined || value === null ? "" : String(value);
    } else if (type === "array") {
        input = document.createElement("textarea");
        input.rows = Math.min(8, Math.max(3, Array.isArray(value) ? value.length + 1 : 4));
        input.value = JSON.stringify(value, null, 2);
    } else if (type === "null") {
        input = document.createElement("input");
        input.type = "text";
        input.placeholder = "null";
        input.value = "";
    } else {
        input = document.createElement("input");
        input.type = "text";
        input.value = value === undefined || value === null ? "" : String(value);
    }

    input.dataset.configPath = encodePath(path);
    input.dataset.configType = type;
    field.appendChild(input);

    if (type === "array") {
        const hint = document.createElement("div");
        hint.className = "config-hint";
        hint.textContent = "请输入有效的 JSON 数组";
        field.appendChild(hint);
    } else if (type === "null") {
        const hint = document.createElement("div");
        hint.className = "config-hint";
        hint.textContent = "留空将保持为 null";
        field.appendChild(hint);
    }

    return field;
}

function setValueByPath(target, path, value) {
    if (!path.length) {
        return;
    }
    let cursor = target;
    for (let index = 0; index < path.length - 1; index += 1) {
        const segment = path[index];
        if (cursor[segment] === undefined) {
            cursor[segment] = {};
        }
        cursor = cursor[segment];
    }
    cursor[path[path.length - 1]] = value;
}

function collectConfigFromForm() {
    const snapshot = JSON.parse(JSON.stringify(currentConfig || {}));

    if (!configForm) {
        return snapshot;
    }

    const fields = configForm.querySelectorAll("[data-config-path]");

    fields.forEach((field) => {
        const rawPath = field.dataset.configPath;
        const type = field.dataset.configType || "string";
        const segments = decodePath(rawPath);
        let value;

        if (type === "boolean") {
            value = field.value === "true";
        } else if (type === "number") {
            const parsed = Number(field.value);
            if (Number.isNaN(parsed)) {
                throw new Error(`字段 ${segments.join(".")} 需要填写数字`);
            }
            value = parsed;
        } else if (type === "array") {
            const text = field.value.trim();
            try {
                const parsed = text ? JSON.parse(text) : [];
                if (!Array.isArray(parsed)) {
                    throw new Error();
                }
                value = parsed;
            } catch (err) {
                throw new Error(`字段 ${segments.join(".")} 需要有效的 JSON 数组`);
            }
        } else if (type === "null") {
            value = field.value.trim() === "" ? null : field.value;
        } else {
            value = field.value;
        }

        setValueByPath(snapshot, segments, value);
    });

    return snapshot;
}

function renderAttachments() {
    if (!attachmentList) {
        return;
    }

    attachmentList.innerHTML = "";

    if (!currentAttachments.length) {
        const empty = document.createElement("li");
        empty.className = "config-hint";
        empty.textContent = "暂无附件";
        attachmentList.appendChild(empty);
    } else {
        currentAttachments.forEach((item) => {
            const li = document.createElement("li");
            li.className = "attachment-item";

            const info = document.createElement("div");
            info.style.display = "flex";
            info.style.flexDirection = "column";
            info.style.flex = "1";
            info.style.gap = "4px";

            const nameSpan = document.createElement("span");
            nameSpan.textContent = item.name;
            info.appendChild(nameSpan);

            const size = document.createElement("small");
            size.textContent = humanFileSize(item.size);
            info.appendChild(size);

            const actions = document.createElement("div");
            const removeButton = document.createElement("button");
            removeButton.className = "secondary";
            removeButton.textContent = "删除";
            removeButton.addEventListener("click", () => deleteAttachment(item.name));
            actions.appendChild(removeButton);

            li.appendChild(info);
            li.appendChild(actions);
            attachmentList.appendChild(li);
        });
    }

    if (attachmentsCount) {
        attachmentsCount.textContent = `${currentAttachments.length} 个附件`;
    }
}

async function loadAttachments() {
    try {
        const response = await fetch("/api/attachments");
        if (!response.ok) {
            throw new Error("读取附件列表失败");
        }
        const data = await response.json();
        currentAttachments = data.attachments || [];
        renderAttachments();
    } catch (err) {
        appendLog("附件", err.message, "error");
    }
}

async function uploadAttachments() {
    if (!attachmentInput || !attachmentInput.files.length) {
        appendLog("附件", "请选择要上传的文件", "warn");
        return;
    }

    const formData = new FormData();
    Array.from(attachmentInput.files).forEach((file) => {
        formData.append("files", file, file.name);
    });

    try {
        const response = await fetch("/api/attachments", {
            method: "POST",
            body: formData,
        });
        if (!response.ok) {
            const error = await response.json().catch(() => ({}));
            throw new Error(error.detail || "上传失败");
        }
        const result = await response.json();
        appendLog(
            "附件",
            `成功上传 ${result.saved?.length || 0} 个文件`,
            "success"
        );
        attachmentInput.value = "";
        loadAttachments();
    } catch (err) {
        appendLog("附件", err.message, "error");
    }
}

async function deleteAttachment(name) {
    if (!name) {
        return;
    }

    try {
        const response = await fetch(
            `/api/attachments/${encodeURIComponent(name)}`,
            { method: "DELETE" }
        );
        if (!response.ok) {
            const error = await response.json().catch(() => ({}));
            throw new Error(error.detail || "删除失败");
        }
        appendLog("附件", `已删除 ${name}`, "warn");
        loadAttachments();
    } catch (err) {
        appendLog("附件", err.message, "error");
    }
}

async function clearAttachments() {
    try {
        const response = await fetch("/api/attachments", { method: "DELETE" });
        if (!response.ok) {
            const error = await response.json().catch(() => ({}));
            throw new Error(error.detail || "清空失败");
        }
        const data = await response.json();
        appendLog(
            "附件",
            `已清空 ${data.deleted || 0} 个文件`,
            "warn"
        );
        loadAttachments();
    } catch (err) {
        appendLog("附件", err.message, "error");
    }
}

function connectWebSocket() {
    const protocol = window.location.protocol === "https:" ? "wss" : "ws";
    socket = new WebSocket(`${protocol}://${window.location.host}/ws/events`);

    socket.onmessage = (event) => {
        try {
            const data = JSON.parse(event.data);
            handleEvent(data);
        } catch (err) {
            console.error("无法解析事件", err);
        }
    };

    socket.onclose = () => {
        if (reconnectTimer) {
            clearTimeout(reconnectTimer);
        }
        reconnectTimer = setTimeout(connectWebSocket, 2000);
    };
}

async function startAgent() {
    try {
        const response = await fetch("/api/start", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ question: questionInput.value }),
        });
        if (!response.ok) {
            const error = await response.json();
            throw new Error(error.detail || "启动失败");
        }
        appendLog("系统", "开始执行任务", "success");
    } catch (err) {
        appendLog("错误", err.message, "error");
    }
}

async function stopAgent() {
    try {
        const response = await fetch("/api/terminate", { method: "POST" });
        if (!response.ok) {
            const error = await response.json();
            throw new Error(error.detail || "终止失败");
        }
        appendLog("系统", "发送终止请求", "warn");
    } catch (err) {
        appendLog("错误", err.message, "error");
    }
}

async function loadConfig() {
    try {
        const response = await fetch("/api/config");
        if (!response.ok) {
            throw new Error("读取配置失败");
        }
        const data = await response.json();
        currentConfig = data.config || {};
        renderConfigForm();
        configSource.textContent = data.source || "config.json";
    } catch (err) {
        appendLog("错误", err.message, "error");
    }
}

async function saveConfig() {
    try {
        const payload = collectConfigFromForm();
        const response = await fetch("/api/config", {
            method: "PUT",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(payload),
        });
        if (!response.ok) {
            const error = await response.json();
            throw new Error(error.detail || "保存失败");
        }
        appendLog("配置", "保存成功", "success");
        currentConfig = payload;
    } catch (err) {
        appendLog("配置错误", err.message, "error");
    }
}

async function respondFlag(approve) {
    try {
        const response = await fetch("/api/flag", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ approve }),
        });
        if (!response.ok) {
            const error = await response.json();
            throw new Error(error.detail || "反馈失败");
        }
        appendLog(
            "Flag 反馈",
            approve ? "已确认" : "已拒绝",
            approve ? "success" : "warn"
        );
    } catch (err) {
        appendLog("Flag 错误", err.message, "error");
    } finally {
        awaitingFlag = false;
        flagDialog.classList.add("hidden");
        flagValue.textContent = "";
    }
}

startButton.addEventListener("click", startAgent);
stopButton.addEventListener("click", stopAgent);
reloadConfigButton.addEventListener("click", loadConfig);
saveConfigButton.addEventListener("click", saveConfig);
clearLogButton.addEventListener("click", () => {
    logStream.innerHTML = "";
});
uploadAttachmentsButton.addEventListener("click", uploadAttachments);
clearAttachmentsButton.addEventListener("click", clearAttachments);
confirmFlagButton.addEventListener("click", () => respondFlag(true));
rejectFlagButton.addEventListener("click", () => respondFlag(false));

window.addEventListener("click", (event) => {
    if (awaitingFlag && event.target === flagDialog) {
        respondFlag(false);
    }
});

connectWebSocket();
loadConfig();
loadAttachments();

fetch("/api/status")
    .then((response) => response.json())
    .then((data) => setStatus(data.status || "idle"))
    .catch(() => setStatus("idle"));
