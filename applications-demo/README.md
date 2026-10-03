# Application Templates

`@<organization>/<name>/manifest.json` 是源码入口，使用
`1flowbase.application-template-archive/v1`。`package` 为可移植定义骨架，
`files` 按路径排序，记录每个 JSON 文件精确 UTF-8 bytes 的 SHA-256。
单字段 `{ "$file": "relative/path.json" }` 可递归引用声明文件；禁止循环、
未声明引用、未引用文件、符号链接及不安全路径。

页面按 page/tab/document 拆分，应用按 metadata/flow 拆分，数据建模定义按 ID 拆分，
MCP tools 按 tool ID SHA-256 的前两位分桶。所有数组顺序与稳定 ID 都保留。
`export.config.json`、README、`catalog-entry.json` 是源目录辅助文件，不进入 ZIP。
目录不能再保留聚合 `template.json`。

## Local Tools

公共模块 `scripts/application-template/archive.mjs` 导出三个 async API：

- `splitPackage(package, directory)` 写入分片及 manifest，返回 manifest。
- `readPackage(directory)` 验证所有声明文件后返回完整可移植 package。
- `buildArchive(directory)` 返回确定性 ZIP 的 Buffer。

ZIP 使用 STORE、字典序、固定 1980-01-01 时间与 0644 regular-file mode；
同一声明内容生成相同 bytes，不依赖本地 mtime。调用方负责使用临时目录和原子替换，
避免留下旧分片。源目录可以包含上述辅助文件，archive 仅打包声明文件与 manifest。

## Release

`package.release.template_id` 必须等于源目录的 `@org/name`；
`release_version` 为正整数，修改内容时递增，同版本不同 checksum 拒绝发布。

```sh
node scripts/application-template/publisher.mjs plan --repo-root .
node scripts/application-template/publisher.mjs sign --directory applications-demo/@org/name \
  --private-key /private/ed25519.pem --key-id official-key \
  --download-url https://github.com/taichuy/1flowbase-official-plugins/releases/download/application-template-org-name-v2/org-name-v2.zip \
  --archive /tmp/org-name-v2.zip --output /tmp/records/org-name-v2.json
node scripts/application-template/publisher.mjs update --repo-root . \
  --records-dir /tmp/records --public-key /private/public.pem
```

推送源目录后 `application-template-catalog` workflow 一键执行上述计划、签名、
发布与更新。使用现有 `OFFICIAL_PLUGIN_SIGNING_PRIVATE_KEY_PEM` 和
`OFFICIAL_PLUGIN_SIGNING_KEY_ID` secrets；无私钥时只提交源码，不伪造签名。
Ed25519 签名覆盖 ZIP 精确 bytes。Release 已存在时下载并逐字节比较，不能覆盖资产。

`releases/v1/catalog.json` 保留已发布版本；`catalog-entry.json` 由已验证签名记录生成，
不手工填写。共享 `extension-catalog/v1` 生成器产生分页和搜索索引，category 为
`applications-demo`，source kind 为 `application_template_release`。
消费者信任预配置公钥，不能从 catalog 自行接受公钥。

```sh
node --test scripts/application-template/_tests/*.test.mjs scripts/_tests/extension-catalog.test.mjs
```
