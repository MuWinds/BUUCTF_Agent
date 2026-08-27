"""提供 load_skill 工具，让 LLM 按需加载专业技能。"""

from typing import Any, Dict

from agent.skill import SkillRegistry
from ctf_tool.base_tool import BaseTool


class LoadSkillTool(BaseTool):
    """按需加载 CTF 专业技能到对话上下文。"""

    def __init__(self, skill_registry: SkillRegistry) -> None:
        """初始化技能加载工具。

        Args:
            skill_registry: 技能注册表实例。
        """
        self._registry = skill_registry

    def execute(self, tool_name: str, arguments: Dict[str, Any]) -> str:
        """加载指定 skill 并返回其内容。

        Args:
            tool_name: 工具名（未使用）。
            arguments: 参数字典，需包含 name。

        Returns:
            skill 内容文本，或错误信息。
        """
        del tool_name
        name = arguments.get("name", "")
        if not name:
            return "错误：未提供 skill 名称"

        skill = self._registry.get(name)
        if not skill:
            available = ", ".join(self._registry.names())
            return f"错误：未找到 skill '{name}'。可用 skills: {available}"

        return (
            f"<skill_content name=\"{skill.name}\">\n"
            f"{skill.content}\n"
            f"</skill_content>"
        )

    @property
    def function_config(self) -> Dict[str, Any]:
        """返回工具函数配置。

        Returns:
            函数调用配置字典。
        """
        # 构建可用 skill 列表用于描述
        skill_list = "\n".join(
            f"- {s.name}: {s.description}"
            for s in self._registry.all()
        )
        description = (
            "加载 CTF 专业技能到上下文中。"
            "当你遇到需要特定领域知识的题目时，"
            "先调用此工具加载相关技能。\n可用技能：\n" + skill_list
        )

        return {
            "type": "function",
            "function": {
                "name": "load_skill",
                "description": description,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "要加载的 skill 名称",
                        }
                    },
                    "required": ["name"],
                },
            },
        }
