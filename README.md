# usuyo-fetch/douyin

抖音的获取插件，供 [usuyo](https://github.com/usuyo/usuyo) 调用：按分享链接取回单个作品，喜欢和收藏由浏览器里的脚本交来。

| 方法 | 清单里的 id | 取得 | 账号 | 层级 | 现状 |
|---|---|---|---|---|---|
| 分享页 | `share-page` | 单个作品：描述、作者、发布时间、计数、音乐信息、封面、图文的图片、视频、原始数据 | 不使用 | `public-web` | 已实现；2026-11-12 实测分享页不再带作品数据，现在取不到（`upstream-changed`） |
| H5 页的令牌 | `h5-token` | 同分享页：单个作品的描述、作者、发布时间、计数、音乐信息、封面、图文的图片、视频、原始数据 | 不使用 | `private-api`，默认关 | 已实现，只在本机假服务器上测过，没有对着平台跑过 |
| 个人信息下载 | `export` | 存过的：喜欢、收藏 | 不使用 | `official-export` | 未实现 |
| 浏览器脚本 | `userscript` | 喜欢、收藏的链接 | 只在使用者的浏览器内 | `browser-session` | 已实现，没在真浏览器里跑过 |
| DouyinDataReview 导出 | `ddr-import` | 喜欢、收藏、观看历史 | 不使用 | `official-export` | 未实现 |
| iPhone 镜像 | `iphone` | 喜欢、收藏 | 只在使用者手机的 App 内 | `vision-ui` | 未实现 |
| 安卓手机 | `android` | 喜欢、收藏 | 只在使用者手机的 App 内 | `vision-ui` | 未实现 |
| MediaCrawler 的结果文件 | `mediacrawler` | 存下的：你自己跑 MediaCrawler 留下的抖音帖子文件（json 或 jsonl，或把 data 文件夹压成的 zip）里每篇的链接、标题、作者昵称、发布时间；不读评论、CSV、SQLite；只列链接，内容交给本插件的其他方法去取 | 不使用（文件是使用者自己用登录了账号的 MediaCrawler 跑出来的，这种方法不联网） | `private-api`，默认关 | 已实现，只对着手写的文件测过，没有对着 MediaCrawler 真正的输出比对过 |

分享页解析移植自 [parse-video](https://github.com/wujunwei928/parse-video)（MIT，Copyright (c) 2022 wujunwei928；`h5-token` 的令牌做法读自它的 PR #106），字段名对照 [ParseHub](https://github.com/z-mio/ParseHub)（MIT，Copyright (c) 2024 梓澪）。
