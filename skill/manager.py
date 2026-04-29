"""
@brief Skill 管理模块：发现、解析和加载 SKILL.md 文件。

参考 opencode 的 skill 系统，适配 BUUCTF_Agent 的 Python 架构。
每个 skill 是一个包含 YAML frontmatter 的 Markdown 文件，
提供 CTF 领域的专业知识和方法论。
"""

import logging
import os
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional

import yaml

logger = logging.getLogger(__name__)


@dataclass
class SkillInfo:
    """Skill 的元数据和内容。"""

    name: str
    description: str
    content: str
    location: str
    tags: List[str] = field(default_factory=list)


class SkillManager:
    """
    @brief 负责 skill 的发现、加载和查询。

    扫描多个目录下的 SKILL.md 文件，解析 YAML frontmatter，
    缓存 skill 内容，并提供格式化输出供 prompt 注入使用。
    """

    # 默认搜索的目录列表
    DEFAULT_SKILL_DIRS = [
        "./skills",
    ]

    def __init__(
        self,
        extra_paths: Optional[List[str]] = None,
    ) -> None:
        """
        @brief 初始化 SkillManager。
        @param extra_paths 额外的 skill 目录路径列表。
        """
        self._skills: Dict[str, SkillInfo] = {}
        self._dirs: set[str] = set()
        self._loaded = False

        for d in self.DEFAULT_SKILL_DIRS:
            self._dirs.add(os.path.abspath(d))

        if extra_paths:
            for p in extra_paths:
                self._dirs.add(os.path.abspath(p))

    def load(self) -> None:
        """
        @brief 扫描所有配置目录并加载 SKILL.md 文件。
        """
        if self._loaded:
            return

        for directory in self._dirs:
            if not os.path.isdir(directory):
                logger.debug("skill 目录不存在，跳过: %s", directory)
                continue
            self._scan_directory(directory)

        self._loaded = True
        logger.info("已加载 %d 个 skill", len(self._skills))

    def _scan_directory(self, directory: str) -> None:
        """
        @brief 递归扫描目录下的所有 SKILL.md 文件。
        @param directory 要扫描的根目录。
        """
        for root, _dirs, files in os.walk(directory):
            for file_name in files:
                if file_name.lower() == "skill.md":
                    file_path = os.path.join(root, file_name)
                    self._load_skill_file(file_path)

    def _load_skill_file(self, file_path: str) -> None:
        """
        @brief 解析单个 SKILL.md 文件并注册到缓存。
        @param file_path SKILL.md 文件的绝对路径。
        """
        try:
            with open(file_path, "r", encoding="utf-8") as f:
                raw = f.read()
        except Exception as error:
            logger.warning("读取 skill 文件失败 %s: %s", file_path, error)
            return

        frontmatter, body = self._parse_frontmatter(raw)
        if frontmatter is None:
            logger.warning("skill 文件缺少有效 frontmatter: %s", file_path)
            return

        name = frontmatter.get("name", "").strip()
        description = frontmatter.get("description", "").strip()

        if not name or not description:
            logger.warning(
                "skill 文件缺少 name 或 description: %s", file_path
            )
            return

        tags = frontmatter.get("tags", [])
        if not isinstance(tags, list):
            tags = []

        skill = SkillInfo(
            name=name,
            description=description,
            content=body.strip(),
            location=file_path,
            tags=tags,
        )

        if name in self._skills:
            logger.warning(
                "skill 名称冲突 '%s'，后者覆盖前者: %s (原: %s)",
                name,
                file_path,
                self._skills[name].location,
            )

        self._skills[name] = skill
        logger.info("已加载 skill: %s (%s)", name, file_path)

    @staticmethod
    def _parse_frontmatter(raw: str) -> tuple[Optional[Dict[str, Any]], str]:
        """
        @brief 从 Markdown 文本中解析 YAML frontmatter 和正文。

        @param raw 原始 Markdown 文本。
        @return (frontmatter 字典, 正文内容)；若无有效 frontmatter 则返回 (None, 原文)。
        """
        match = re.match(r"^---\s*\n(.*?)\n---\s*\n", raw, re.DOTALL)
        if not match:
            return None, raw

        yaml_str = match.group(1)
        body = raw[match.end():]

        try:
            data = yaml.safe_load(yaml_str)
        except yaml.YAMLError as error:
            logger.warning("YAML 解析失败: %s", error)
            return None, raw

        if not isinstance(data, dict):
            return None, raw

        return data, body

    def get_all(self) -> List[SkillInfo]:
        """
        @brief 获取所有已加载的 skill 列表。
        @return SkillInfo 列表，按名称排序。
        """
        self.load()
        return sorted(self._skills.values(), key=lambda s: s.name)

    def get(self, name: str) -> Optional[SkillInfo]:
        """
        @brief 根据名称获取 skill。
        @param name skill 名称。
        @return SkillInfo 或 None。
        """
        self.load()
        return self._skills.get(name)

    def get_names(self) -> List[str]:
        """
        @brief 获取所有 skill 名称列表。
        @return 名称列表。
        """
        self.load()
        return sorted(self._skills.keys())

    def format_for_prompt(self, selected: Optional[List[str]] = None) -> str:
        """
        @brief 将 skill 内容格式化为可注入 prompt 的文本。

        @param selected 要包含的 skill 名称列表。None 表示全部。
        @return 格式化的 skill 文本。
        """
        self.load()

        skills_to_include = self.get_all()
        if selected:
            skills_to_include = [
                s for s in skills_to_include if s.name in selected
            ]

        if not skills_to_include:
            return ""

        parts: List[str] = []
        parts.append(
            "以下是一些 CTF 领域的专业知识和方法论，"
            "可能对解题有帮助：\n"
        )

        for skill in skills_to_include:
            part = f"## {skill.name}"
            if skill.tags:
                part += f" [{', '.join(skill.tags)}]"
            part += f"\n{skill.description}\n"
            if skill.content:
                part += f"\n{skill.content}\n"
            parts.append(part)

        return "\n".join(parts)

    def format_list_for_display(self) -> str:
        """
        @brief 将 skill 列表格式化为终端显示文本。
        @return 格式化的列表文本。
        """
        self.load()
        skills = self.get_all()
        if not skills:
            return "未找到任何 skill。"

        lines: List[str] = []
        lines.append(f"共 {len(skills)} 个可用 skill:\n")
        for skill in skills:
            tags_str = f" [{', '.join(skill.tags)}]" if skill.tags else ""
            lines.append(f"  - {skill.name}{tags_str}")
            lines.append(f"    {skill.description}")
            lines.append(f"    位置: {skill.location}")
            lines.append("")

        return "\n".join(lines)
