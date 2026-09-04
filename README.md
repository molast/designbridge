# DesignBridge

DesignBridge 是一个用于抓取蓝湖设计稿原图的 Tauri 桌面工具。粘贴蓝湖分享链接后，应用会
打开独立授权窗口，复用该窗口的蓝湖登录态读取画板列表，并将原图与项目元数据保存到应用
数据目录。蓝湖接口探测、设计 JSON 解析、资源下载和图片转换均由 Rust 端完成，WebView
只负责蓝湖页面渲染和登录。

## 开发

```bash
pnpm install
pnpm tauri dev
```

也可以在项目根目录直接运行一键启动脚本：

```bash
./start.sh
# 或
pnpm start
```

脚本会自动切换到项目目录，并在依赖缺失时执行 `pnpm install`。

当前版本支持蓝湖 UI 设计项目的项目名、画板名称、尺寸、更新时间和原始设计图抓取；会从
设计 JSON 中识别一个目标图标，并在 `mipmap-xxhdpi` 目录输出 WebP（源图按 3/4 缩放）。
登录信息只保存在系统 WebView 的站点数据中，不会写入抓取结果。

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
