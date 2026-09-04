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

当前版本支持蓝湖 UI 设计项目的项目名、画板名称、尺寸、更新时间、原始设计图、可检查图层
和有效切图抓取，并在 `mipmap-xxhdpi` 目录输出 WebP。登录信息只保存在系统 WebView 的
站点数据中，不会写入抓取结果。

## MCP（第一版）

项目包含一个基于 STDIO 的只读 MCP 服务，供 Codex 或其他 MCP 客户端读取已经抓取的设计稿、图层、截图和切图。服务不要求 Tauri 窗口保持打开，也不会再次请求蓝湖；它直接读取 DesignBridge 的应用数据目录：

- macOS：`~/Library/Application Support/com.designbridge.app/captures`
- 其他系统：系统应用数据目录下的 `com.designbridge.app/captures`

运行服务：

```bash
./mcp.sh
# 或
pnpm mcp
```

如需使用其他抓取目录，可以设置 `DESIGNBRIDGE_CAPTURES_DIR`：

```bash
DESIGNBRIDGE_CAPTURES_DIR=/path/to/captures ./mcp.sh
```

### 配置 Codex

在项目根目录执行以下命令，将本地服务注册到 Codex：

```bash
codex mcp add designbridge -- /绝对路径/DesignBridge/mcp.sh
```

也可以在 Codex 配置中加入：

```toml
[mcp_servers.designbridge]
command = "/绝对路径/DesignBridge/mcp.sh"
startup_timeout_sec = 30
tool_timeout_sec = 60
```

### 工具和调用顺序

服务提供 7 个只读工具：

- `list_designs`：列出本机已抓取的设计稿和稳定链接。
- `resolve_design`：解析 `designbridge://` 或蓝湖原始链接，返回画板尺寸、图层数和切图数。
- `get_design_context`：按 `nodeId`、设计坐标 `rect` 或浅层级树读取图层 frame、文本和样式。
- `find_layers`：按设计坐标点、矩形或文本查找图层；点查询会优先返回有切图的图层。
- `get_design_screenshot`：返回整张设计稿，或按设计坐标裁剪后的截图。
- `get_assets`：返回指定图层关联的切图元数据和图片内容。
- `get_comments`：返回设计稿评论的作者、正文、版本、设计坐标、状态、回复，以及评论位置命中的候选 UI 图层。

实现 UI 时建议先调用 `resolve_design`，再用用户提供的局部坐标调用 `find_layers` 或 `get_design_context`。需要视觉核对时调用 `get_design_screenshot`，设计评审信息使用 `get_comments`，最后只针对实际需要的图层调用 `get_assets`。所有 frame 和裁剪坐标均使用返回的设计坐标空间（当前 Android 设计稿为 `dp`）。

设计稿工具栏中的“复制 MCP 链接”按钮会复制当前画板的稳定链接，例如：

```text
designbridge://design/{projectId}/{imageId}
```

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
