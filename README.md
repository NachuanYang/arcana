# 秘仪 Arcana

秘仪是一款本地运行的塔罗牌体验应用，包含牌组与牌阵、抽牌记录和智能解读流程。

## 目录

- src/：Vue界面与前端业务逻辑。
- src-tauri/：桌面端服务、数据持久化和安全边界。
- data/：78张牌的结构化内容。
- public/assets/tarot/：应用使用的牌面图片及来源元数据。

## 开发

需要Node.js/npm与Rust工具链。

    npm install
    npm run dev

桌面开发与构建：

    npm run tauri -- dev
    npm run tauri -- build

前端检查构建与单元测试：

    npm run build
    npm test

## Optional evaluation fixtures

`tools/eval/corpus/` contains synthetic JSONL cases used by the optional Rust live-evaluation test feature. Captured evaluation responses and reviewer results are excluded from this source distribution.
