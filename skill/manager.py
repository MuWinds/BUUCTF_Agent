"""Skill 管理模块：发现、解析和加载 SKILL.md 文件。

参考 crush 的 skill 系统设计（agentskills.io 开放标准）。
每个 skill 是一个包含 YAML frontmatter 的 Markdown 文件，
frontmatter 仅需 name 和 description，正文为 agent 指令。
"""

import logging
import os
import re
from dataclasses import dataclass
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


class SkillManager:
    """负责 skill 的发现、加载和查询。

    扫描多个目录下的 SKILL.md 文件，解析 YAML frontmatter，
    缓存 skill 内容，并提供格式化输出供 prompt 注入使用。
    """

    DEFAULT_SKILL_DIRS = [
        "./skills",
    ]

    def __init__(
        self,
        extra_paths: Optional[List[str]] = None,
    ) -> None:
        self._skills: Dict[str, SkillInfo] = {}
        self._dirs: set[str] = set()
        self._loaded = False

        for d in self.DEFAULT_SKILL_DIRS:
            self._dirs.add(os.path.abspath(d))

        if extra_paths:
            for p in extra_paths:
                self._dirs.add(os.path.abspath(p))

    def load(self) -> None:
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
        for root, _dirs, files in os.walk(directory):
            for file_name in files:
                if file_name.lower() == "skill.md":
                    file_path = os.path.join(root, file_name)
                    self._load_skill_file(file_path)

    def _load_skill_file(self, file_path: str) -> None:
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

        skill = SkillInfo(
            name=name,
            description=description,
            content=body.strip(),
            location=file_path,
        )

        if name in self._skills:
            logger.warning(
                "skill 名称冲突 '%s'，后者覆盖前者: %s (原: %s)",
                name,
                file_path,
                self._skills[name].location,
            )

        self._skills[name] = skill
        logger.info("已加载 skill: %s", name)

    @staticmethod
    def _parse_frontmatter(raw: str) -> tuple[Optional[Dict[str, Any]], str]:
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
        self.load()
        return sorted(self._skills.values(), key=lambda s: s.name)

    def get(self, name: str) -> Optional[SkillInfo]:
        self.load()
        return self._skills.get(name)

    def get_names(self) -> List[str]:
        self.load()
        return sorted(self._skills.keys())

    def format_for_prompt(self, selected: Optional[List[str]] = None) -> str:
        """将 skill 内容格式化为可注入 prompt 的文本。

        参考 crush 的 ToPromptXML 设计，以 XML 标签包裹每个 skill。
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
        parts.append("以下是你可调用的 CTF 领域专业能力：")

        for skill in skills_to_include:
            part = f"<skill name=\"{skill.name}\">"
            part += f"\n  <description>{skill.description}</description>"

            if skill.content:
                part += f"\n  <instructions>\n{skill.content}\n  </instructions>"

            part += "\n</skill>"
            parts.append(part)

        return "\n\n".join(parts)

