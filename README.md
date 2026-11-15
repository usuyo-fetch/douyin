# usuyo-fetch/douyin

抖音的获取插件，供 [usuyo](https://github.com/usuyo/usuyo) 调用：按分享链接取回单个作品，喜欢和收藏由浏览器里的脚本交来。

| 功能 | 实现方式（方法 id） | 账号 | 风险层级 | 状态 |
|---|---|---|---|---|
| 按链接取单条内容（作品、图文） | `share-page` | 不用 | `public-web`，默认开 | 已实现（对着本机假服务测过）；2026-11-12 实测分享页不再带作品数据，现在取不到（`upstream-changed`） |
| 按链接取单条内容（作品、图文） | `h5-token` | 不用 | `private-api`，默认关 | 已实现（只在本机假服务器上测过，没有对着平台跑过） |
| 列出我的喜欢、收藏 | `export` | 不用 | `official-export`，默认开 | 未实现 |
| 列出我的喜欢、收藏 | `userscript` | 只在你的浏览器里 | `browser-session`，默认开 | 已实现（脚本测试对着手写样例，没在真浏览器里跑过） |
| 列出我的喜欢、收藏 | `iphone` | 你手机的 App 内 | `vision-ui`，默认开 | 未实现 |
| 列出我的喜欢、收藏 | `android` | 你手机的 App 内 | `vision-ui`，默认开 | 未实现 |

视频文件：`share-page` 和 `h5-token` 取页面给的播放地址，当前以取到内容为先，不追画质；高清视频是试验性的，还没有实现。

## 方法

### `share-page`

`fetch`：读分享页，取单个作品的描述、作者、发布时间、计数、音乐信息、封面、图文的图片、视频和原始数据。

### `h5-token`

`fetch`：取得的内容同 `share-page`。照 H5 页自己的做法，用分享页算出的令牌调 iteminfo；令牌的加密方式由平台定，改了就失效。

### `export`

`list`：平台的「个人信息下载」里的喜欢、收藏。未实现。

### `userscript`

不在插件里跑：脚本在你的浏览器里把喜欢、收藏的链接交给 usuyo。

### `iphone`、`android`

`list`：从你手机上的 App 里读喜欢、收藏。未实现。

## 开发

`Cargo.toml` 里的 `usuyo-plugin`、`usuyo-plugin-kit` 是对 `https://github.com/usuyo/usuyo` 的 git 依赖，钉在固定的 `rev`。

本地开发时用旁边 `usuyo/` 的工作区代替它们：`usuyo-fetch/.cargo/config.toml`（不进任何插件仓库）写了下面的覆盖，cargo 从当前目录往上找这个文件，在插件文件夹里跑 cargo 就生效；从别处跑要加 `--config <这个文件>`。

```toml
[patch."https://github.com/usuyo/usuyo"]
usuyo-plugin = { path = "../usuyo/crates/usuyo-plugin" }
usuyo-plugin-kit = { path = "../usuyo/crates/usuyo-plugin-kit" }
```

`Cargo.lock` 记的是覆盖后的路径形式，所以 CI 不加 `--locked` 构建，cargo 只重新锁这两个 usuyo crate。
