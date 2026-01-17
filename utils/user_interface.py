from abc import ABC, abstractmethod
from typing import Any, Optional, List, Dict


class UserInterface(ABC):
    """用户交互抽象接口，定义了所有需要的用户交互方法"""
    
    @abstractmethod
    def confirm_flag(self, flag_candidate: str) -> bool:
        """让用户确认flag是否正确
        
        Args:
            flag_candidate: 候选flag
            
        Returns:
            用户确认结果（True/False）
        """
        pass
    
    @abstractmethod
    def select_mode(self) -> bool:
        """让用户选择运行模式
        
        Returns:
            是否选择自动模式（True/False）
        """
        pass
    
    @abstractmethod
    def input_question_ready(self, prompt: str) -> None:
        """等待用户输入问题准备就绪
        
        Args:
            prompt: 提示信息
        """
        pass
    
    @abstractmethod
    def display_message(self, message: str) -> None:
        """显示消息给用户
        
        Args:
            message: 要显示的消息
        """
        pass
    
    @abstractmethod
    def manual_approval(self, think: str, tool_calls: Any) -> tuple[bool, tuple[str, Any]]:
        """手动模式下获取用户批准
        
        Args:
            think: 思考过程
            tool_calls: 工具调用信息
            
        Returns:
            (是否批准, (思考过程, 工具调用信息))
        """
        pass
    
    @abstractmethod
    def manual_approval_step(self, think: str, tool_calls: List[Dict]) -> tuple[bool, tuple[str, List[Dict]]]:
        """手动模式下的完整批准流程
        
        Args:
            think: 思考过程
            tool_calls: 工具调用信息
            
        Returns:
            (是否批准, (思考过程, 工具调用信息))
        """
        pass


class CommandLineInterface(UserInterface):
    """命令行交互实现"""
    
    def confirm_flag(self, flag_candidate: str) -> bool:
        """让用户确认flag是否正确"""
        print(f"\n发现flag：\n{flag_candidate}")
        print("请确认这个flag是否正确？")

        while True:
            response = input("输入 'y' 确认正确，输入 'n' 表示不正确: ").strip().lower()
            if response == "y":
                return True
            elif response == "n":
                return False
            else:
                print("无效输入，请输入 'y' 或 'n'")
    
    def select_mode(self) -> bool:
        """让用户选择运行模式"""
        print("\n请选择运行模式:")
        print("1. 自动模式（Agent自动生成和执行所有命令）")
        print("2. 手动模式（每一步需要用户批准）")

        while True:
            choice = input("请输入选项编号: ").strip()
            if choice == "1":
                return True
            elif choice == "2":
                return False
            else:
                print("无效选项，请重新选择")
    
    def input_question_ready(self, prompt: str) -> None:
        """等待用户输入问题准备就绪"""
        input(prompt)
    
    def display_message(self, message: str) -> None:
        """显示消息给用户"""
        print(message)
    
    def manual_approval(self, think: str, tool_calls: Any) -> tuple[bool, tuple[str, Any]]:
        """手动模式下获取用户批准"""
        # 这里简化实现，实际需要根据tool_calls的具体结构来显示
        print(f"\n思考过程: {think}")
        print(f"工具调用: {tool_calls}")
        
        while True:
            response = input("是否批准执行？(y/n): ").strip().lower()
            if response == "y":
                return True, (think, tool_calls)
            elif response == "n":
                return False, (think, tool_calls)
            else:
                print("无效输入，请输入 'y' 或 'n'")
    
    def manual_approval_step(self, think: str, tool_calls: List[Dict]) -> tuple[bool, tuple[str, List[Dict]]]:
        """手动模式下的完整批准流程"""
        while True:
            # 显示所有计划调用的工具
            print(f"\n计划调用 {len(tool_calls)} 个工具:")
            for i, tool_call in enumerate(tool_calls):
                print(f"{i+1}. {tool_call.get('tool_name')}: {tool_call.get('arguments')}")
            print()

            print("1. 批准并执行所有工具")
            print("2. 提供反馈并重新思考")
            print("3. 终止解题")
            choice = input("请输入选项编号: ").strip()

            if choice == "1":
                return True, (think, tool_calls)
            elif choice == "2":
                feedback = input("请提供改进建议: ").strip()
                return False, (think, tool_calls, feedback)  # 特殊标记，返回反馈
            elif choice == "3":
                return False, None
            else:
                print("无效选项，请重新选择")