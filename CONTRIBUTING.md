# 贡献指南 / Contributing

感谢你愿意为 RareBox Desktop 贡献代码。

## 1. 提交前必须完成的两件事

所有 Pull Request 必须同时满足：

1. **每个 commit 都要 Sign-off（DCO 1.1）**；
2. **同意贡献者许可协议（CLA）**，把该贡献的不可撤销商用授权授予项目所有者。

缺少任意一项的 PR 不会被合并。

## 2. DCO Sign-off

使用 `git commit -s` 自动添加：

```text
Signed-off-by: Your Name <you@example.com>
```

也可以手动在 commit message 末尾追加一行。该行必须与提交者信息一致，并代表你同意
[Developer Certificate of Origin 1.1](https://developercertificate.org/)：

```text
By making a contribution to this project, I certify that:

(a) The contribution was created in whole or in part by me and I
    have the right to submit it under the open source license
    indicated in the file; or

(b) The contribution is based upon previous work that, to the best
    of my knowledge, is covered under an appropriate open source
    license and I have the right under that license to submit that
    work with modifications, whether created in whole or in part
    by me, under the same open source license (unless I am
    permitted to submit under a different license), as indicated
    in the file; or

(c) The contribution was provided directly to me by some other
    person who certified (a), (b) or (c) and I have not modified
    it.

(d) I understand and agree that this project and the contribution
    are public and that a record of the contribution (including all
    personal information I submit with it, including my sign-off) is
    maintained indefinitely and may be redistributed consistent with
    this project or the open source license(s) involved.
```

## 3. 贡献者许可协议（CLA）

在提交 PR 之前，请阅读 [`CLA.md`](CLA.md)。提交 PR 即表示你同意该协议，特别是：

- 你授予 RareBox Desktop 项目所有者（GitHub [@Genouka](https://github.com/Genouka)）
  一份**不可撤销、全球范围、永久、免费、可再许可、可转让**的许可，允许其使用、
  复制、修改、分发你的贡献，并**以任何许可证（包括商业许可证）再许可**该贡献；
- 你的贡献同时按本仓库的 AGPL-3.0 许可证向公众提供。

如果你代表雇主或其他实体贡献代码，请确保你有权代表该实体授予上述许可。

## 4. 开发流程

```bash
cargo check
cargo test
cargo fmt
```

平台相关构建：

```bash
# Android
./script/package_android.sh

# macOS
./script/package_macos.sh

# iOS（未签名）
./script/package_ios.sh

# Linux
./script/package_linux.sh

# Windows
./script/package_windows.ps1
```

## 5. 提交内容要求

- 不要提交密钥、令牌、证书私钥、日志、本地数据库、AI 提示词或代理指令文件；
- 不要提交 `target/`、`dist/`、`promo/`、`项目说明书.md` 等本地/内部内容；
- 新增第三方代码时，请同时补充其许可证与来源说明。

## 6. 许可证

提交即表示你同意你的贡献按 [AGPL-3.0-only](LICENSE) 发布，并按
[`CLA.md`](CLA.md) 授予项目所有者商用再许可权。
