import litellm
import json
from typing import List, Dict

class Memory:
    def __init__(self, config: dict, max_steps: int = 10, compression_threshold: int = 5):
        """
        记忆管理类，负责历史记录的存储和压缩
        :param config: 配置字典
        :param max_steps: 最大保存步骤数
        :param compression_threshold: 触发压缩的步骤阈值
        """
        self.config = config
        self.max_steps = max_steps
        self.compression_threshold = compression_threshold
        self.history: List[Dict] = []
        self.compressed_memory: str = ""  # 压缩后的记忆摘要
        
    def add_step(self, step: Dict) -> None:
        """添加新的步骤到历史记录"""
        self.history.append(step)
        
        # 检查是否需要压缩记忆
        if len(self.history) >= self.compression_threshold:
            self.compress_memory()
    
    def compress_memory(self) -> None:
        """压缩历史记录，生成摘要"""
        print("压缩记忆中...")
        if not self.history:
            return
            
        # 构建压缩提示
        prompt = (
            "你是一个CTF解题助手，需要压缩解题历史记录。"
            "请生成一个简洁的摘要，保留关键命令、重要发现和当前解题状态。"
            "重点保留有结果输出的步骤，避免包含不必要的细节：\n\n"
        )
        
        for i, step in enumerate(self.history):
            prompt += f"步骤 {i+1}:\n"
            prompt += f"- 目的: {step.get('purpose', '未指定目的')}\n"
            prompt += f"- 命令: {step['content']}\n"
            prompt += f"- 输出摘要: {step['output'][:200]}...\n"
            if "analysis" in step:
                analysis = step['analysis'].get('analysis', '无分析结果')
                prompt += f"- 分析: {analysis[:200]}...\n"
            prompt += "\n"
        
        # 调用LLM生成摘要
        try:
            response = litellm.completion(
                model=self.config['model'],
                api_key=self.config['api_key'],
                api_base=self.config['api_base'],
                messages=[{"role": "user", "content": prompt}],
                max_tokens=512
            )
            
            # 更新压缩记忆
            self.compressed_memory = response.choices[0].message.content.strip()
        except Exception as e:
            print(f"记忆压缩失败: {str(e)}")
            self.compressed_memory = "压缩失败: " + str(e)
        
        # 清空历史记录，保留最后几步
        keep_last = min(3, len(self.history))  # 保留最后3步细节
        self.history = self.history[-keep_last:]
    
    def get_summary(self) -> str:
        """获取记忆摘要，包括压缩记忆和最近几步的细节"""
        summary = ""
        
        if self.compressed_memory:
            summary += f"压缩记忆摘要:\n{self.compressed_memory}\n\n"
        
        if self.history:
            summary += "最近步骤细节:\n"
            for i, step in enumerate(self.history):
                step_num = len(self.history) - i  # 倒序显示，最近的在前面
                summary += f"步骤 {step_num}:\n"
                summary += f"- 目的: {step.get('purpose', '未指定目的')}\n"
                summary += f"- 命令: {step['content']}\n"
                summary += f"- 输出摘要: {step['output'][:200]}...\n"
                if "analysis" in step:
                    analysis = step['analysis'].get('analysis', '无分析结果')
                    summary += f"- 分析: {analysis[:200]}...\n"
                summary += "\n"
        
        return summary if summary else "无历史记录"