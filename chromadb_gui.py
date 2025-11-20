import tkinter as tk
import json
import chromadb
import litellm
import os
import logging
import glob
from typing import List
from tkinter import ttk, messagebox, scrolledtext,filedialog
from chromadb.config import Settings
from config import Config

logger = logging.getLogger(__name__)


class ChromaDBManager:
    """ChromaDB GUI管理器"""

    def __init__(self, root: tk.Tk):
        self.root = root
        self.config = Config.load_config()
        self.root.title("ChromaDB 管理器")
        self.root.geometry("1200x800")

        # ChromaDB客户端
        self.client = None
        self.current_collection = None

        # 初始化UI
        self.setup_ui()

        # 连接到数据库
        self.connect_to_db()

    def setup_ui(self):
        """设置用户界面"""
        # 主框架
        main_frame = ttk.Frame(self.root, padding="10")
        main_frame.grid(row=0, column=0, sticky=(tk.W, tk.E, tk.N, tk.S))

        # 配置网格权重
        self.root.columnconfigure(0, weight=1)
        self.root.rowconfigure(0, weight=1)
        main_frame.columnconfigure(1, weight=1)
        main_frame.rowconfigure(1, weight=1)

        # 数据库连接框架
        conn_frame = ttk.LabelFrame(main_frame, text="数据库连接", padding="5")
        conn_frame.grid(
            row=0, column=0, columnspan=2, sticky=(tk.W, tk.E), pady=(0, 10)
        )

        ttk.Label(conn_frame, text="数据库路径:").grid(row=0, column=0, sticky=tk.W)
        self.db_path_var = tk.StringVar(
            value=self.config.get("persist_directory", "./chromadb")
        )
        ttk.Entry(conn_frame, textvariable=self.db_path_var, width=50).grid(
            row=0, column=1, padx=5
        )
        ttk.Button(conn_frame, text="连接", command=self.connect_to_db).grid(
            row=0, column=2
        )

        # 左侧框架 - 集合管理
        left_frame = ttk.LabelFrame(main_frame, text="集合管理", padding="5")
        left_frame.grid(row=1, column=0, sticky=(tk.W, tk.E, tk.N, tk.S), padx=(0, 10))
        left_frame.columnconfigure(0, weight=1)
        left_frame.rowconfigure(1, weight=1)

        # 集合操作按钮
        collection_btn_frame = ttk.Frame(left_frame)
        collection_btn_frame.grid(row=0, column=0, sticky=(tk.W, tk.E), pady=(0, 5))

        ttk.Button(
            collection_btn_frame, text="刷新集合", command=self.refresh_collections
        ).pack(side=tk.LEFT, padx=(0, 5))
        ttk.Button(
            collection_btn_frame, text="创建集合", command=self.create_collection_dialog
        ).pack(side=tk.LEFT, padx=(0, 5))
        ttk.Button(
            collection_btn_frame, text="删除集合", command=self.delete_collection
        ).pack(side=tk.LEFT)

        # 集合列表
        self.collection_tree = ttk.Treeview(
            left_frame, columns=("count",), show="tree headings", height=15
        )
        self.collection_tree.grid(row=1, column=0, sticky=(tk.W, tk.E, tk.N, tk.S))

        self.collection_tree.heading("#0", text="集合名称")
        self.collection_tree.heading("count", text="文档数量")
        self.collection_tree.column("#0", width=200)
        self.collection_tree.column("count", width=80)

        # 绑定选择事件
        self.collection_tree.bind("<<TreeviewSelect>>", self.on_collection_select)

        # 右侧框架 - 文档管理
        right_frame = ttk.LabelFrame(main_frame, text="文档管理", padding="5")
        right_frame.grid(row=1, column=1, sticky=(tk.W, tk.E, tk.N, tk.S))
        right_frame.columnconfigure(0, weight=1)
        right_frame.rowconfigure(1, weight=1)

        # 文档操作按钮
        doc_btn_frame = ttk.Frame(right_frame)
        doc_btn_frame.grid(row=0, column=0, sticky=(tk.W, tk.E), pady=(0, 5))

        ttk.Button(doc_btn_frame, text="刷新文档", command=self.refresh_documents).pack(
            side=tk.LEFT, padx=(0, 5)
        )
        ttk.Button(
            doc_btn_frame, text="添加文档", command=self.add_document_dialog
        ).pack(side=tk.LEFT, padx=(0, 5))
        ttk.Button(
            doc_btn_frame, text="编辑文档", command=self.edit_document_dialog
        ).pack(side=tk.LEFT, padx=(0, 5))
        ttk.Button(doc_btn_frame, text="删除文档", command=self.delete_document).pack(
            side=tk.LEFT, padx=(0, 5)
        )
        ttk.Button(
            doc_btn_frame, text="搜索文档", command=self.search_documents_dialog
        ).pack(side=tk.LEFT, padx=(0, 5))
        # 添加导入目录按钮
        ttk.Button(
            doc_btn_frame, text="从知识库文件目录中导入", command=self.import_directory_dialog
        ).pack(side=tk.LEFT)

        # 文档列表
        self.document_tree = ttk.Treeview(
            right_frame,
            columns=("metadata", "content_preview"),
            show="tree headings",
            height=15,
        )
        self.document_tree.grid(row=1, column=0, sticky=(tk.W, tk.E, tk.N, tk.S))

        self.document_tree.heading("#0", text="文档ID")
        self.document_tree.heading("metadata", text="元数据")
        self.document_tree.heading("content_preview", text="内容预览")

        self.document_tree.column("#0", width=150)
        self.document_tree.column("metadata", width=200)
        self.document_tree.column("content_preview", width=400)

        # 文档详情框架
        detail_frame = ttk.LabelFrame(main_frame, text="文档详情", padding="5")
        detail_frame.grid(
            row=2, column=0, columnspan=2, sticky=(tk.W, tk.E, tk.N, tk.S), pady=(10, 0)
        )
        detail_frame.columnconfigure(0, weight=1)
        detail_frame.rowconfigure(0, weight=1)

        # 详情文本区域
        self.detail_text = scrolledtext.ScrolledText(detail_frame, width=100, height=10)
        self.detail_text.grid(row=0, column=0, sticky=(tk.W, tk.E, tk.N, tk.S))

        # 绑定文档选择事件
        self.document_tree.bind("<<TreeviewSelect>>", self.on_document_select)

    def connect_to_db(self):
        """连接到数据库"""
        try:
            db_path = self.db_path_var.get()
            self.client = chromadb.PersistentClient(
                path=db_path, settings=Settings(anonymized_telemetry=False)
            )
            messagebox.showinfo("成功", f"已连接到数据库: {db_path}")
            self.refresh_collections()
        except Exception as e:
            messagebox.showerror("错误", f"连接数据库失败: {str(e)}")

    def refresh_collections(self):
        """刷新集合列表"""
        if not self.client:
            return

        try:
            # 清空当前列表
            for item in self.collection_tree.get_children():
                self.collection_tree.delete(item)

            # 获取所有集合
            collections = self.client.list_collections()

            for collection in collections:
                # 获取集合中的文档数量
                try:
                    count = collection.count()
                    self.collection_tree.insert(
                        "",
                        "end",
                        text=collection.name,
                        values=(count,),
                        iid=collection.name,
                    )
                except:
                    self.collection_tree.insert(
                        "",
                        "end",
                        text=collection.name,
                        values=("N/A",),
                        iid=collection.name,
                    )

        except Exception as e:
            messagebox.showerror("错误", f"刷新集合列表失败: {str(e)}")

    def create_collection_dialog(self):
        """创建集合对话框"""
        dialog = tk.Toplevel(self.root)
        dialog.title("创建集合")
        dialog.geometry("400x200")
        dialog.transient(self.root)
        dialog.grab_set()

        ttk.Label(dialog, text="集合名称:").grid(
            row=0, column=0, sticky=tk.W, padx=5, pady=5
        )
        name_var = tk.StringVar()
        ttk.Entry(dialog, textvariable=name_var).grid(
            row=0, column=1, sticky=(tk.W, tk.E), padx=5, pady=5
        )

        ttk.Label(dialog, text="集合描述:").grid(
            row=1, column=0, sticky=tk.W, padx=5, pady=5
        )
        desc_var = tk.StringVar()
        ttk.Entry(dialog, textvariable=desc_var).grid(
            row=1, column=1, sticky=(tk.W, tk.E), padx=5, pady=5
        )

        dialog.columnconfigure(1, weight=1)

        def create():
            name = name_var.get().strip()
            desc = desc_var.get().strip()
            if not name:
                messagebox.showerror("错误", "请输入集合名称")
                return

            try:
                self.client.create_collection(
                    name=name, metadata={"description": desc} if desc else None
                )
                messagebox.showinfo("成功", f"集合 '{name}' 创建成功")
                self.refresh_collections()
                dialog.destroy()
            except Exception as e:
                messagebox.showerror("错误", f"创建集合失败: {str(e)}")

        btn_frame = ttk.Frame(dialog)
        btn_frame.grid(row=2, column=0, columnspan=2, pady=20)

        ttk.Button(btn_frame, text="创建", command=create).pack(side=tk.LEFT, padx=5)
        ttk.Button(btn_frame, text="取消", command=dialog.destroy).pack(
            side=tk.LEFT, padx=5
        )

    def delete_collection(self):
        """删除选中的集合"""
        selection = self.collection_tree.selection()
        if not selection:
            messagebox.showwarning("警告", "请先选择一个集合")
            return

        collection_name = selection[0]
        if not messagebox.askyesno("确认", f"确定要删除集合 '{collection_name}' 吗？"):
            return

        try:
            self.client.delete_collection(name=collection_name)
            messagebox.showinfo("成功", f"集合 '{collection_name}' 已删除")
            self.refresh_collections()
            self.current_collection = None
        except Exception as e:
            messagebox.showerror("错误", f"删除集合失败: {str(e)}")

    def on_collection_select(self, event):
        """选择集合事件"""
        selection = self.collection_tree.selection()
        if not selection:
            return

        collection_name = selection[0]
        try:
            self.current_collection = self.client.get_collection(name=collection_name)
            self.refresh_documents()
        except Exception as e:
            messagebox.showerror("错误", f"获取集合失败: {str(e)}")

    def refresh_documents(self):
        """刷新文档列表"""
        if not self.current_collection:
            return

        try:
            # 清空当前列表
            for item in self.document_tree.get_children():
                self.document_tree.delete(item)

            # 获取所有文档（限制数量避免性能问题）
            results = self.current_collection.get(limit=1000)

            for i, doc_id in enumerate(results["ids"]):
                content = results["documents"][i]
                metadata = results["metadatas"][i] if results["metadatas"] else {}

                # 内容预览（前50个字符）
                content_preview = content[:50] + "..." if len(content) > 50 else content
                metadata_str = (
                    json.dumps(metadata, ensure_ascii=False)[:30] + "..."
                    if metadata
                    else "{}"
                )

                self.document_tree.insert(
                    "",
                    "end",
                    text=doc_id,
                    values=(metadata_str, content_preview),
                    iid=doc_id,
                )

        except Exception as e:
            messagebox.showerror("错误", f"刷新文档列表失败: {str(e)}")

    def add_document_dialog(self):
        """添加文档对话框"""
        if not self.current_collection:
            messagebox.showwarning("警告", "请先选择一个集合")
            return

        dialog = tk.Toplevel(self.root)
        dialog.title("添加文档")
        dialog.geometry("600x500")
        dialog.transient(self.root)
        dialog.grab_set()

        # 文档ID
        ttk.Label(dialog, text="文档ID:").grid(
            row=0, column=0, sticky=tk.W, padx=5, pady=5
        )
        id_var = tk.StringVar()
        ttk.Entry(dialog, textvariable=id_var).grid(
            row=0, column=1, sticky=(tk.W, tk.E), padx=5, pady=5
        )

        # 元数据
        ttk.Label(dialog, text="元数据 (JSON格式):").grid(
            row=1, column=0, sticky=tk.W, padx=5, pady=5
        )
        metadata_text = scrolledtext.ScrolledText(dialog, height=5)
        metadata_text.grid(row=1, column=1, sticky=(tk.W, tk.E), padx=5, pady=5)
        metadata_text.insert("1.0", "{}")

        # 内容
        ttk.Label(dialog, text="内容:").grid(
            row=2, column=0, sticky=tk.W, padx=5, pady=5
        )
        content_text = scrolledtext.ScrolledText(dialog, height=10)
        content_text.grid(
            row=2, column=1, sticky=(tk.W, tk.E, tk.N, tk.S), padx=5, pady=5
        )

        dialog.columnconfigure(1, weight=1)
        dialog.rowconfigure(2, weight=1)

        def add_document():
            doc_id = id_var.get().strip()
            metadata_str = metadata_text.get("1.0", tk.END).strip()
            content = content_text.get("1.0", tk.END).strip()

            if not doc_id or not content:
                messagebox.showerror("错误", "文档ID和内容不能为空")
                return

            try:
                metadata = json.loads(metadata_str) if metadata_str else {}
            except json.JSONDecodeError:
                messagebox.showerror("错误", "元数据格式错误，必须是有效的JSON")
                return

            try:
                self.current_collection.add(
                    embeddings=self.get_embeddings([content]),
                    documents=[content], metadatas=[metadata], ids=[doc_id]
                )
                messagebox.showinfo("成功", "文档添加成功")
                self.refresh_documents()
                dialog.destroy()
            except Exception as e:
                messagebox.showerror("错误", f"添加文档失败: {str(e)}")

        btn_frame = ttk.Frame(dialog)
        btn_frame.grid(row=3, column=0, columnspan=2, pady=20)

        ttk.Button(btn_frame, text="添加", command=add_document).pack(
            side=tk.LEFT, padx=5
        )
        ttk.Button(btn_frame, text="取消", command=dialog.destroy).pack(
            side=tk.LEFT, padx=5
        )

    def edit_document_dialog(self):
        """编辑文档对话框"""
        selection = self.document_tree.selection()
        if not selection:
            messagebox.showwarning("警告", "请先选择一个文档")
            return

        doc_id = selection[0]

        try:
            # 获取文档详情
            results = self.current_collection.get(ids=[doc_id])
            if not results["documents"]:
                messagebox.showerror("错误", "文档不存在")
                return

            content = results["documents"][0]
            metadata = results["metadatas"][0] if results["metadatas"] else {}

            dialog = tk.Toplevel(self.root)
            dialog.title("编辑文档")
            dialog.geometry("600x500")
            dialog.transient(self.root)
            dialog.grab_set()

            ttk.Label(dialog, text=f"文档ID: {doc_id}").grid(
                row=0, column=0, sticky=tk.W, padx=5, pady=5
            )

            # 元数据
            ttk.Label(dialog, text="元数据 (JSON格式):").grid(
                row=1, column=0, sticky=tk.W, padx=5, pady=5
            )
            metadata_text = scrolledtext.ScrolledText(dialog, height=5)
            metadata_text.grid(row=1, column=1, sticky=(tk.W, tk.E), padx=5, pady=5)
            metadata_text.insert(
                "1.0", json.dumps(metadata, ensure_ascii=False, indent=2)
            )

            # 内容
            ttk.Label(dialog, text="内容:").grid(
                row=2, column=0, sticky=tk.W, padx=5, pady=5
            )
            content_text = scrolledtext.ScrolledText(dialog, height=10)
            content_text.grid(
                row=2, column=1, sticky=(tk.W, tk.E, tk.N, tk.S), padx=5, pady=5
            )
            content_text.insert("1.0", content)

            dialog.columnconfigure(1, weight=1)
            dialog.rowconfigure(2, weight=1)

            def update_document():
                metadata_str = metadata_text.get("1.0", tk.END).strip()
                new_content = content_text.get("1.0", tk.END).strip()

                if not new_content:
                    messagebox.showerror("错误", "内容不能为空")
                    return

                try:
                    new_metadata = json.loads(metadata_str) if metadata_str else {}
                except json.JSONDecodeError:
                    messagebox.showerror("错误", "元数据格式错误，必须是有效的JSON")
                    return

                try:
                    # 先删除再添加（更新）
                    self.current_collection.delete(ids=[doc_id])
                    self.current_collection.add(
                        embeddings=self.get_embeddings([new_content]),
                        documents=[new_content],
                        metadatas=[new_metadata],
                        ids=[doc_id],
                    )
                    messagebox.showinfo("成功", "文档更新成功")
                    self.refresh_documents()
                    dialog.destroy()
                except Exception as e:
                    messagebox.showerror("错误", f"更新文档失败: {str(e)}")

            btn_frame = ttk.Frame(dialog)
            btn_frame.grid(row=3, column=0, columnspan=2, pady=20)

            ttk.Button(btn_frame, text="更新", command=update_document).pack(
                side=tk.LEFT, padx=5
            )
            ttk.Button(btn_frame, text="取消", command=dialog.destroy).pack(
                side=tk.LEFT, padx=5
            )

        except Exception as e:
            messagebox.showerror("错误", f"获取文档失败: {str(e)}")

    def delete_document(self):
        """删除选中的文档"""
        selection = self.document_tree.selection()
        if not selection:
            messagebox.showwarning("警告", "请先选择一个文档")
            return

        doc_id = selection[0]
        if not messagebox.askyesno("确认", f"确定要删除文档 '{doc_id}' 吗？"):
            return

        try:
            self.current_collection.delete(ids=[doc_id])
            messagebox.showinfo("成功", "文档删除成功")
            self.refresh_documents()
        except Exception as e:
            messagebox.showerror("错误", f"删除文档失败: {str(e)}")

    def search_documents_dialog(self):
        """搜索文档对话框"""
        if not self.current_collection:
            messagebox.showwarning("警告", "请先选择一个集合")
            return

        dialog = tk.Toplevel(self.root)
        dialog.title("搜索文档")
        dialog.geometry("400x200")
        dialog.transient(self.root)
        dialog.grab_set()

        ttk.Label(dialog, text="搜索查询:").grid(
            row=0, column=0, sticky=tk.W, padx=5, pady=5
        )
        query_var = tk.StringVar()
        ttk.Entry(dialog, textvariable=query_var, width=50).grid(
            row=0, column=1, sticky=(tk.W, tk.E), padx=5, pady=5
        )

        ttk.Label(dialog, text="返回结果数量:").grid(
            row=1, column=0, sticky=tk.W, padx=5, pady=5
        )
        n_results_var = tk.StringVar(value="10")
        ttk.Entry(dialog, textvariable=n_results_var, width=10).grid(
            row=1, column=1, sticky=tk.W, padx=5, pady=5
        )

        dialog.columnconfigure(1, weight=1)

        def search():
            query = query_var.get().strip()
            if not query:
                messagebox.showerror("错误", "请输入搜索查询")
                return

            try:
                n_results = int(n_results_var.get())
            except ValueError:
                messagebox.showerror("错误", "结果数量必须是整数")
                return

            try:
                results = self.current_collection.query(
                    query_embeddings=self.get_embeddings([query]),
                    query_texts=[query], n_results=n_results
                )

                # 显示搜索结果
                self.show_search_results(results, query)
                dialog.destroy()

            except Exception as e:
                messagebox.showerror("错误", f"搜索失败: {str(e)}")

        btn_frame = ttk.Frame(dialog)
        btn_frame.grid(row=2, column=0, columnspan=2, pady=20)

        ttk.Button(btn_frame, text="搜索", command=search).pack(side=tk.LEFT, padx=5)
        ttk.Button(btn_frame, text="取消", command=dialog.destroy).pack(
            side=tk.LEFT, padx=5
        )

    def get_embeddings(self, texts: List[str]) -> List[List[float]]:
        """获取文本的嵌入向量"""
        try:
            response = litellm.embedding(
                input=texts,
                model=self.config["llm"]["embedding"]["model"],
                api_base=self.config["llm"]["embedding"]["api_base"],
                api_key=self.config["llm"]["embedding"]["api_key"],
            )
            return [item["embedding"] for item in response.data]
        except Exception as e:
            logger.error(f"获取嵌入向量失败: {e}")
            raise

    def show_search_results(self, results, query):
        """显示搜索结果"""
        result_window = tk.Toplevel(self.root)
        result_window.title(f"搜索结果: {query}")
        result_window.geometry("800x600")

        text_area = scrolledtext.ScrolledText(result_window, wrap=tk.WORD)
        text_area.pack(fill=tk.BOTH, expand=True, padx=10, pady=10)

        output = f"搜索查询: {query}\n"
        output += "=" * 50 + "\n\n"

        if results and results["documents"]:
            for i, doc in enumerate(results["documents"][0]):
                output += f"结果 {i+1}:\n"
                output += f"距离: {results['distances'][0][i] if results['distances'] else 'N/A'}\n"
                output += f"内容: {doc}\n"
                if results["metadatas"] and results["metadatas"][0]:
                    output += f"元数据: {json.dumps(results['metadatas'][0][i], ensure_ascii=False, indent=2)}\n"
                output += "-" * 50 + "\n\n"
        else:
            output += "未找到相关结果\n"

        text_area.insert("1.0", output)
        text_area.config(state=tk.DISABLED)

    def on_document_select(self, event):
        """选择文档事件"""
        selection = self.document_tree.selection()
        if not selection or not self.current_collection:
            return

        doc_id = selection[0]

        try:
            results = self.current_collection.get(ids=[doc_id])
            if results["documents"]:
                content = results["documents"][0]
                metadata = results["metadatas"][0] if results["metadatas"] else {}

                detail_text = f"文档ID: {doc_id}\n"
                detail_text += "=" * 50 + "\n"
                detail_text += (
                    f"元数据:\n{json.dumps(metadata, ensure_ascii=False, indent=2)}\n\n"
                )
                detail_text += f"内容:\n{content}"

                self.detail_text.config(state=tk.NORMAL)
                self.detail_text.delete("1.0", tk.END)
                self.detail_text.insert("1.0", detail_text)
                self.detail_text.config(state=tk.DISABLED)

        except Exception as e:
            self.detail_text.config(state=tk.NORMAL)
            self.detail_text.delete("1.0", tk.END)
            self.detail_text.insert("1.0", f"获取文档详情失败: {str(e)}")
            self.detail_text.config(state=tk.DISABLED)

    # 添加导入目录对话框方法
    def import_directory_dialog(self):
        """导入目录对话框"""
        if not self.current_collection:
            messagebox.showwarning("警告", "请先选择一个集合")
            return

        # 选择目录
        directory = filedialog.askdirectory(title="选择包含JSON文件的目录")
        if not directory:
            return

        # 创建进度对话框
        progress_dialog = tk.Toplevel(self.root)
        progress_dialog.title("导入进度")
        progress_dialog.geometry("400x150")
        progress_dialog.transient(self.root)
        progress_dialog.grab_set()

        ttk.Label(progress_dialog, text="正在导入文件，请稍候...").pack(pady=10)
        
        progress_var = tk.DoubleVar()
        progress_bar = ttk.Progressbar(
            progress_dialog, variable=progress_var, maximum=100, length=300
        )
        progress_bar.pack(pady=10)
        
        status_label = ttk.Label(progress_dialog, text="准备导入...")
        status_label.pack(pady=5)

        # 在后台执行导入
        self.root.after(100, lambda: self.import_directory_files(
            directory, progress_dialog, progress_var, status_label
        ))

    def import_directory_files(self, directory, progress_dialog, progress_var, status_label):
        """导入目录中的所有JSON文件"""
        try:
            # 查找所有JSON文件
            json_files = glob.glob(os.path.join(directory, "*.json"))
            total_files = len(json_files)
            
            if total_files == 0:
                messagebox.showwarning("警告", "目录中没有找到JSON文件")
                progress_dialog.destroy()
                return

            imported_count = 0
            error_count = 0
            all_documents = []
            all_metadatas = []
            all_ids = []

            for i, json_file in enumerate(json_files):
                filename = os.path.basename(json_file)
                status_label.config(text=f"正在处理: {filename}")
                progress_var.set((i / total_files) * 100)
                self.root.update_idletasks()

                try:
                    with open(json_file, 'r', encoding='utf-8') as f:
                        data = json.load(f)
                    
                    if not isinstance(data, list):
                        logger.warning(f"文件 {filename} 不是有效的JSON数组，跳过")
                        continue

                    for j, item in enumerate(data):
                        if not isinstance(item, dict):
                            continue
                        
                        title = item.get("title", "").strip()
                        content = item.get("content", "").strip()
                        
                        if not content:
                            continue
                        
                        # 生成文档ID
                        doc_id = f"{os.path.splitext(filename)[0]}_{j}"
                        
                        # 准备元数据
                        metadata = {"source_file": filename}
                        if title:
                            metadata["title"] = title
                        
                        all_documents.append(content)
                        all_metadatas.append(metadata)
                        all_ids.append(doc_id)
                        
                        # 分批处理，避免内存溢出
                        if len(all_documents) >= 50:
                            self._add_batch_documents(all_documents, all_metadatas, all_ids)
                            imported_count += len(all_documents)
                            all_documents, all_metadatas, all_ids = [], [], []

                    imported_count += len(all_documents)

                except Exception as e:
                    logger.error(f"处理文件 {filename} 时出错: {e}")
                    error_count += 1
                    continue

            # 添加剩余文档
            if all_documents:
                self._add_batch_documents(all_documents, all_metadatas, all_ids)

            # 更新进度完成
            progress_var.set(100)
            status_label.config(text="导入完成!")
            self.root.update_idletasks()
            
            # 显示结果摘要
            messagebox.showinfo(
                "导入完成", 
                f"导入完成!\n\n"
                f"成功导入: {imported_count} 个文档\n"
                f"错误文件: {error_count} 个\n"
                f"总文件数: {total_files} 个"
            )
            
            # 关闭进度对话框并刷新文档列表
            progress_dialog.destroy()
            self.refresh_documents()

        except Exception as e:
            logger.error(f"导入目录时出错: {e}")
            messagebox.showerror("错误", f"导入目录失败: {str(e)}")
            progress_dialog.destroy()

    def _add_batch_documents(self, documents, metadatas, ids):
        """批量添加文档到集合"""
        if not documents:
            return
            
        try:
            embeddings = self.get_embeddings(documents)
            self.current_collection.add(
                embeddings=embeddings,
                documents=documents,
                metadatas=metadatas,
                ids=ids
            )
        except Exception as e:
            logger.error(f"批量添加文档失败: {e}")
            raise

def main():
    """主函数"""
    root = tk.Tk()
    _ = ChromaDBManager(root)
    root.mainloop()


if __name__ == "__main__":
    main()
