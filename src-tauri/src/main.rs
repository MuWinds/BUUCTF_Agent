// Windows release 构建时不弹出额外的控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    coding_agent_lib::run()
}
