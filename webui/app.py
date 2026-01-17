import sys
import os

# 将项目根目录添加到Python路径
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from flask import Flask, render_template, request, jsonify
from utils.user_interface import UserInterface
from agent.workflow import Workflow
from config import Config
import logging


app = Flask(__name__)
app.secret_key = os.urandom(24)

# 配置日志
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

# 加载配置
config = Config.load_config()

# 存储当前的工作流和用户接口实例
import queue
from threading import Lock

# 使用锁保护共享数据
workflow_lock = Lock()
user_interface_lock = Lock()

# 存储当前的工作流和用户接口实例
workflows = {}
user_interfaces = {}

# 用于存储等待用户输入的事件
waiting_events = {}
waiting_events_lock = Lock()


class WebInterface(UserInterface):
    """Web界面交互实现"""
    
    def __init__(self, session_id):
        self.session_id = session_id
        self.flag_candidate = None
        self.think = None
        self.tool_calls = None
        
        # 创建队列用于线程间通信
        self.flag_queue = queue.Queue()
        self.mode_queue = queue.Queue()
        self.approval_queue = queue.Queue()
        self.feedback_queue = queue.Queue()
    
    def confirm_flag(self, flag_candidate: str) -> bool:
        """让用户确认flag是否正确"""
        # 存储flag候选值
        self.flag_candidate = flag_candidate
        
        # 更新会话状态
        with workflow_lock:
            if self.session_id in workflows:
                workflows[self.session_id]['flag_candidate'] = flag_candidate
                workflows[self.session_id]['waiting_for'] = 'flag_confirm'
        
        # 等待前端确认
        try:
            result = self.flag_queue.get(timeout=300)  # 5分钟超时
            return result
        except queue.Empty:
            return False
    
    def select_mode(self) -> bool:
        """让用户选择运行模式"""
        # 更新会话状态
        with workflow_lock:
            if self.session_id in workflows:
                workflows[self.session_id]['waiting_for'] = 'mode_select'
        
        # 等待前端选择
        try:
            result = self.mode_queue.get(timeout=300)  # 5分钟超时
            return result
        except queue.Empty:
            return False  # 默认返回False（手动模式）
    
    def input_question_ready(self, prompt: str) -> None:
        """等待用户输入问题准备就绪"""
        # 在Web界面中，这个方法由用户在前端直接完成
        pass
    
    def display_message(self, message: str) -> None:
        """显示消息给用户"""
        # 消息会存储在会话中，供前端获取
        with workflow_lock:
            if self.session_id not in workflows:
                workflows[self.session_id] = {}
            if 'messages' not in workflows[self.session_id]:
                workflows[self.session_id]['messages'] = []
            workflows[self.session_id]['messages'].append(message)
    
    def manual_approval(self, think: str, tool_calls: any) -> tuple[bool, tuple[str, any]]:
        """手动模式下获取用户批准"""
        self.think = think
        self.tool_calls = tool_calls
        
        # 更新会话状态
        with workflow_lock:
            if self.session_id in workflows:
                workflows[self.session_id]['waiting_for'] = 'manual_approval'
                workflows[self.session_id]['think'] = think
                workflows[self.session_id]['tool_calls'] = tool_calls
        
        # 等待前端批准
        try:
            result = self.approval_queue.get(timeout=300)  # 5分钟超时
            return result
        except queue.Empty:
            return False, (think, tool_calls)
    
    def manual_approval_step(self, think: str, tool_calls: list) -> tuple[bool, tuple[str, list]]:
        """手动模式下的完整批准流程"""
        self.think = think
        self.tool_calls = tool_calls
        
        # 更新会话状态
        with workflow_lock:
            if self.session_id in workflows:
                workflows[self.session_id]['waiting_for'] = 'manual_approval_step'
                workflows[self.session_id]['think'] = think
                workflows[self.session_id]['tool_calls'] = tool_calls
        
        # 等待前端批准
        try:
            result = self.approval_queue.get(timeout=300)  # 5分钟超时
            return result
        except queue.Empty:
            return False, (think, tool_calls)


@app.route('/')
def index():
    """主页面"""
    return render_template('index.html')


@app.route('/api/start', methods=['POST'])
def start_solve():
    """开始解题"""
    data = request.json
    question = data.get('question')
    session_id = data.get('session_id')
    
    if not question or not session_id:
        return jsonify({'error': '缺少必要参数'}), 400
    
    try:
        # 创建Web用户接口
        web_interface = WebInterface(session_id)
        user_interfaces[session_id] = web_interface
        
        # 创建工作流
        workflow = Workflow(config=config, user_interface=web_interface)
        workflows[session_id] = {
            'workflow': workflow,
            'messages': [],
            'question': question,
            'flag_candidate': None,
            'mode_selection': None,
            'approval_request': None
        }
        
        # 启动解题过程（异步）
        import threading
        def solve_task():
            try:
                result = workflow.solve(question)
                workflows[session_id]['result'] = result
            except Exception as e:
                logger.error(f"解题过程出错: {str(e)}")
                workflows[session_id]['error'] = str(e)
        
        threading.Thread(target=solve_task).start()
        return jsonify({'status': 'success', 'message': '开始解题'})
    except Exception as e:
        logger.error(f"启动解题出错: {str(e)}")
        return jsonify({'error': str(e)}), 500


@app.route('/api/messages/<session_id>')
def get_messages(session_id):
    """获取会话消息和状态"""
    if session_id not in workflows:
        return jsonify({
            'messages': [],
            'status': 'session_not_found'
        })
    
    with workflow_lock:
        session_data = workflows[session_id]
        messages = session_data.get('messages', [])
        waiting_for = session_data.get('waiting_for', None)
        flag_candidate = session_data.get('flag_candidate', None)
        mode_selection = session_data.get('mode_selection', None)
        approval_request = session_data.get('approval_request', None)
        think = session_data.get('think', None)
        tool_calls = session_data.get('tool_calls', None)
        result = session_data.get('result', None)
        error = session_data.get('error', None)
    
    return jsonify({
        'messages': messages,
        'waiting_for': waiting_for,
        'flag_candidate': flag_candidate,
        'think': think,
        'tool_calls': tool_calls,
        'result': result,
        'error': error
    })


@app.route('/api/history/<session_id>')
def get_history(session_id):
    if session_id not in workflows:
        return jsonify({'history': [], 'status': 'session_not_found'})
    with workflow_lock:
        session_data = workflows[session_id]
        workflow = session_data.get('workflow')
        agent = getattr(workflow, 'agent', None)
        memory = getattr(agent, 'memory', None)
        history = getattr(memory, 'history', []) if memory else []
    limit_param = request.args.get('limit', None)
    try:
        limit = int(limit_param) if limit_param is not None else None
    except ValueError:
        limit = None
    steps = history[-limit:] if limit else history
    def simplify_step(s):
        return {
            'step': s.get('step'),
            'think': s.get('think'),
            'tool_calls': s.get('tool_calls', []),
            'output': s.get('output'),
            'raw_outputs': s.get('raw_outputs'),
            'analysis': s.get('analysis'),
            'status': s.get('status')
        }
    return jsonify({
        'question': session_data.get('question'),
        'history': [simplify_step(s) for s in steps],
        'waiting_for': session_data.get('waiting_for'),
        'flag_candidate': session_data.get('flag_candidate'),
        'think': session_data.get('think'),
        'tool_calls': session_data.get('tool_calls'),
        'result': session_data.get('result'),
        'error': session_data.get('error')
    })


@app.route('/api/flag_confirm', methods=['POST'])
def flag_confirm():
    """确认flag"""
    data = request.json
    session_id = data.get('session_id')
    confirm = data.get('confirm', False)
    
    if session_id not in user_interfaces:
        return jsonify({'error': '会话不存在'}), 404
    
    # 获取用户接口
    user_interface = user_interfaces[session_id]
    
    # 将结果放入队列
    user_interface.flag_queue.put(confirm)
    
    # 更新会话状态
    with workflow_lock:
        if session_id in workflows:
            workflows[session_id]['waiting_for'] = None
            workflows[session_id]['flag_candidate'] = None
    
    return jsonify({'status': 'success', 'message': 'flag确认已处理'})


@app.route('/api/mode_select', methods=['POST'])
def mode_select():
    """选择模式"""
    data = request.json
    session_id = data.get('session_id')
    auto_mode = data.get('auto_mode', False)
    
    if session_id not in user_interfaces:
        return jsonify({'error': '会话不存在'}), 404
    
    # 获取用户接口
    user_interface = user_interfaces[session_id]
    
    # 将结果放入队列
    user_interface.mode_queue.put(auto_mode)
    
    # 更新会话状态
    with workflow_lock:
        if session_id in workflows:
            workflows[session_id]['waiting_for'] = None
            workflows[session_id]['auto_mode'] = auto_mode
    
    return jsonify({'status': 'success', 'message': '模式选择已处理'})


@app.route('/api/approval', methods=['POST'])
def approval():
    """手动批准"""
    data = request.json
    session_id = data.get('session_id')
    approve = data.get('approve', False)
    feedback = data.get('feedback', '')
    
    if session_id not in user_interfaces:
        return jsonify({'error': '会话不存在'}), 404
    
    # 获取用户接口
    user_interface = user_interfaces[session_id]
    
    # 如果批准，返回批准结果；否则，返回反馈供 Agent 再思考
    if approve:
        user_interface.approval_queue.put((True, (user_interface.think, user_interface.tool_calls)))
    else:
        if feedback:
            # 将原思考、工具计划和用户反馈一起交给 Agent，生成新的方案
            user_interface.approval_queue.put(
                (False, (user_interface.think, user_interface.tool_calls, feedback))
            )
        else:
            user_interface.approval_queue.put((False, (user_interface.think, user_interface.tool_calls)))
    
    # 更新会话状态
    with workflow_lock:
        if session_id in workflows:
            workflows[session_id]['waiting_for'] = None
            workflows[session_id]['think'] = None
            workflows[session_id]['tool_calls'] = None
    
    return jsonify({'status': 'success', 'message': '批准已处理'})


if __name__ == '__main__':
    app.run(debug=True, host='0.0.0.0', port=5000)
