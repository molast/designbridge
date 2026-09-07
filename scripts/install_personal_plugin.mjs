#!/usr/bin/env node

import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const pluginName = "designbridge";
const repoRoot = path.resolve(process.argv[2] || ".");
const source = path.join(repoRoot, "plugins", pluginName);
const userHome = process.env.DESIGNBRIDGE_PLUGIN_HOME
  ? path.resolve(process.env.DESIGNBRIDGE_PLUGIN_HOME)
  : os.homedir();
const target = path.join(userHome, "plugins", pluginName);
const marketplacePath = path.join(userHome, ".agents", "plugins", "marketplace.json");

function fail(message) {
  console.error(message);
  process.exit(1);
}

function readJson(filePath) {
  try {
    return JSON.parse(fs.readFileSync(filePath, "utf8"));
  } catch (error) {
    fail(`无法读取 JSON 文件：${filePath} (${error.message})`);
  }
}

if (!fs.existsSync(path.join(source, ".codex-plugin", "plugin.json"))) {
  fail(`找不到 DesignBridge 插件包：${source}`);
}

fs.mkdirSync(path.dirname(target), { recursive: true });
fs.cpSync(source, target, { recursive: true, force: true });
fs.rmSync(path.join(target, ".designbridge-root"), { force: true });

const mcpBinarySource = process.env.DESIGNBRIDGE_MCP_BINARY
  ? path.resolve(process.env.DESIGNBRIDGE_MCP_BINARY)
  : null;
if (mcpBinarySource) {
  if (!fs.existsSync(mcpBinarySource)) {
    fail(`找不到 DesignBridge MCP 运行文件：${mcpBinarySource}`);
  }
  const mcpBinaryTarget = path.join(target, "bin", "designbridge-mcp");
  fs.mkdirSync(path.dirname(mcpBinaryTarget), { recursive: true });
  fs.copyFileSync(mcpBinarySource, mcpBinaryTarget);
  fs.chmodSync(mcpBinaryTarget, fs.statSync(mcpBinaryTarget).mode | 0o111);
  console.log(`MCP 运行文件已安装到：${mcpBinaryTarget}`);
}

const launcher = path.join(target, "scripts", "mcp.sh");
if (fs.existsSync(launcher)) {
  fs.chmodSync(launcher, fs.statSync(launcher).mode | 0o111);
}

fs.mkdirSync(path.dirname(marketplacePath), { recursive: true });
const marketplace = fs.existsSync(marketplacePath)
  ? readJson(marketplacePath)
  : {
      name: "personal",
      interface: { displayName: "Personal" },
      plugins: [],
    };

if (!marketplace || typeof marketplace !== "object" || Array.isArray(marketplace)) {
  fail(`个人插件市场配置必须是 JSON 对象：${marketplacePath}`);
}
if (typeof marketplace.name !== "string" || !marketplace.name.trim()) {
  fail(`个人插件市场配置缺少有效的 name：${marketplacePath}`);
}
if (!Array.isArray(marketplace.plugins)) {
  fail(`个人插件市场配置的 plugins 必须是数组：${marketplacePath}`);
}

const entry = {
  name: pluginName,
  source: {
    source: "local",
    path: `./plugins/${pluginName}`,
  },
  policy: {
    installation: "AVAILABLE",
    authentication: "ON_INSTALL",
  },
  category: "Developer Tools",
};
const existingIndex = marketplace.plugins.findIndex(
  (item) => item && typeof item === "object" && item.name === pluginName,
);
if (existingIndex >= 0) marketplace.plugins[existingIndex] = entry;
else marketplace.plugins.push(entry);

fs.writeFileSync(marketplacePath, `${JSON.stringify(marketplace, null, 2)}\n`, "utf8");
console.log(`插件文件已安装到：${target}`);
console.log(`个人插件市场已更新：${marketplacePath}`);
console.log(`MARKETPLACE_NAME=${marketplace.name}`);
console.log(`PLUGIN_DIR=${target}`);
