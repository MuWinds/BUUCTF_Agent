"""
@brief Skill 发现与加载模块。

@details
参考 OpenCode 的 skill 系统设计。每个 skill 是一个 SKILL.md 文件，
包含 YAML frontmatter（name, description）和 Markdown 正文内容。
支持从项目本地目录和全局目录发现 skills。
"""

import logging
import os
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional

import yaml

logger = logging.getLogger(__name__)

# skill 名称校验：小写字母+数字+连字符
SKILL_NAME_RE = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")


@dataclass
class SkillInfo:
    """单个 skill 的元数据和内容。"""
    name: str
    description: str
    content: str
    location: str  # SKILL.md 的绝对路径


@dataclass
class SkillRegistry:
    """Skill 注册表，负责发现和管理所有可用 skills。"""
    skills: Dict[str, SkillInfo] = field(default_factory=dict)
    dirs: List[str] = field(default_factory=list)

    def get(self, name: str) -> Optional[SkillInfo]:
        """按名称获取 skill。"""
        return self.skills.get(name)

    def all(self) -> List[SkillInfo]:
        """返回所有已注册的 skills，按名称排序。"""
        return sorted(self.skills.values(), key=lambda s: s.name)

    def names(self) -> List[str]:
        """返回所有 skill 名称列表。"""
        return sorted(self.skills.keys())


def discover_skills(
    config: Optional[Dict[str, Any]] = None,
    project_dir: str = ".",
) -> SkillRegistry:
    """
    @brief 从多个目录发现并加载所有 skills。
    @param config 全局配置字典，用于读取 skills.paths。
    @param project_dir 项目根目录。
    @return SkillRegistry 实例。
    """
    registry = SkillRegistry()
    search_dirs = _get_search_dirs(config, project_dir)

    for search_dir in search_dirs:
        _scan_dir(search_dir, registry)

    logger.info("共发现 %d 个 skills: %s", len(registry.skills), registry.names())
    return registry


def _get_search_dirs(
    config: Optional[Dict[str, Any]],
    project_dir: str,
) -> List[str]:
    """构建 skill 搜索目录列表。"""
    dirs = []

    # 1. 项目本地目录
    project_local = os.path.join(project_dir, ".buuctf_agent", "skills")
    if os.path.isdir(project_local):
        dirs.append(project_local)

    # 2. 全局目录
    home = Path.home()
    global_dir = home / ".buuctf_agent" / "skills"
    if global_dir.is_dir():
        dirs.append(str(global_dir))

    # 3. 配置自定义路径
    if config:
        skills_config = config.get("skills", {})
        custom_paths = skills_config.get("paths", [])
        for p in custom_paths:
            expanded = os.path.expanduser(p)
            if not os.path.isabs(expanded):
                expanded = os.path.join(project_dir, expanded)
            if os.path.isdir(expanded):
                dirs.append(expanded)

    return dirs


def _scan_dir(base_dir: str, registry: SkillRegistry) -> None:
    """扫描目录下的所有 */SKILL.md 文件。"""
    if base_dir in registry.dirs:
        return
    registry.dirs.append(base_dir)

    for entry in os.listdir(base_dir):
        skill_dir = os.path.join(base_dir, entry)
        if not os.path.isdir(skill_dir):
            continue
        skill_md = os.path.join(skill_dir, "SKILL.md")
        if not os.path.isfile(skill_md):
            continue
        _load_skill_file(skill_md, registry)


def _load_skill_file(path: str, registry: SkillRegistry) -> None:
    """解析单个 SKILL.md 文件并注册。"""
    try:
        with open(path, "r", encoding="utf-8") as f:
            raw = f.read()
    except OSError as error:
        logger.warning("读取 skill 文件失败 %s: %s", path, error)
        return

    # 分离 YAML frontmatter 和 Markdown 正文
    parts = raw.split("---", 2)
    if len(parts) < 3:
        logger.warning("skill 文件缺少 YAML frontmatter: %s", path)
        return

    try:
        frontmatter = yaml.safe_load(parts[1])
    except yaml.YAMLError as error:
        logger.warning("skill frontmatter 解析失败 %s: %s", path, error)
        return

    if not isinstance(frontmatter, dict):
        logger.warning("skill frontmatter 不是有效字典: %s", path)
        return

    name = frontmatter.get("name", "")
    description = frontmatter.get("description", "")
    content = parts[2].strip()

    if not name or not description:
        logger.warning("skill 缺少 name 或 description: %s", path)
        return

    if not SKILL_NAME_RE.match(name):
        logger.warning("skill 名称格式无效 '%s': %s", name, path)
        return

    if name in registry.skills:
        logger.warning("skill 名称重复 '%s'，忽略: %s", name, path)
        return

    registry.skills[name] = SkillInfo(
        name=name,
        description=description,
        content=content,
        location=os.path.abspath(path),
    )
    logger.info("已加载 skill: %s (%s)", name, path)
