# Fixture notes

All files here are hand-written with invented content. NOT MEASURED: nobody involved ran DouyinDataReview or had a real export file, so none of this has been compared with one.

- Shape read from ZhangYukwiver/DouyinDataReview (MIT, v1.47.1, read 2026-11-12 through the GitHub API, nothing run): `collector/dataExport.mjs` (`buildDataExport`: `schemaVersion: 2`, `exportedAt`, `source`, `parts`, `updatedAt`, `records.{watch_history,liked_videos,favorite_videos}`, `chatConversations`, `chatMessages`, `warnings`; an optional `folder` on records), `src/domain/personalRecords.ts` (record fields: `id`, `title`, `author`, `occurredAt`, `url`, `videoId`, `occurredAtSource` of `platform_action` / `archive_action` / `unknown`, `publishedAt`, `mediaType`), `collector/normalizer.mjs` (how `occurredAt` is filled: for watch history the history view time; for likes and favorites only `play_progress.last_modified_time`, the last play time, never the like or favorite time; ids `watch_history:<id>:<time>`, `liked_videos:<id>`; the placeholder title `未命名视频`; live records have `videoId` `live-<anchor id>`). The export file name `抖音数据_<date>_<time>.json` is from `App.tsx`. Only field names were read; no code was copied.
- `export.json`: one watch-history video watched twice (the later time wins), one with no time and the placeholder title, one from the official archive (`archive_action`), one live record (skipped); a like with a time that must not become `actedAt`, a like of the same video as the first watch; a favorite of an image post.
- `chat-only.json`: an export with only chat selected, so the three arrays are empty.
- `changed.json`: records whose ids are unusable (the format changed).
- `newer-version.json`: `schemaVersion` 3.
- The `_fixture` key is not part of the tool's format; the plugin ignores it.
