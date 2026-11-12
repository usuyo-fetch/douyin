// ==UserScript==
// @name         usuyo：交出抖音的收藏和喜欢
// @namespace    https://github.com/usuyo-fetch/douyin
// @version      0.1.0
// @description  在你自己的抖音网页版「喜欢」「收藏」页上，只看页面自己请求回来的列表，记下作品链接、标题、作者和时间；点「交给 usuyo」时先确认本机 127.0.0.1:47615 上的是 usuyo，再把清单交过去，你在 usuyo 的导入页确认。没记到东西时，面板上的「复制诊断」会把回答的结构（字段名、类型、长度，不含任何内容）复制给你，由你自己决定给不给我们。不对抖音发额外请求、不自动滚动、不读 cookie，除了本机的 usuyo 不往别处发。
// @match        https://www.douyin.com/user/self*
// @run-at       document-start
// @noframes
// @grant        unsafeWindow
// @grant        GM_getValue
// @grant        GM_setValue
// @grant        GM_xmlhttpRequest
// @grant        GM_registerMenuCommand
// @connect      127.0.0.1
// @license      MIT
// ==/UserScript==

/*
 * 接口和字段名取自公开的项目，只读了名字和回答的形状，没有抄代码；没有一份是在有账号的页面上量过的，
 * 所以字段都按「有就用、没有就跳过」处理，条目没有 aweme_id 就不记。
 *  - Hannah2227/douyin-favorite-MyLike（MIT）：喜欢 aweme/v1/web/aweme/favorite/，回答 aweme_list、has_more、max_cursor；
 *    收藏 aweme/v1/web/aweme/listcollection/，回答 aweme_list、has_more、cursor；收藏夹 collects/list/（collects_list，
 *    collects_id_str、collects_name）和 collects/video/list/（请求带 collects_id）；条目 aweme_id、desc、author.nickname、
 *    create_time、images（有图的是图文）；收藏夹页在 /user/self?from_tab_name=main&showTab=favorite_collection；
 *    回答外层的 status_code，0 是成功。
 *  - ZhangYukwiver/DouyinDataReview（MIT）：条目里喜欢的时间是 digg_time、收藏的时间是 collect_time；
 *    它的喜欢示例把回答包在 data 里，这里两种都认。
 *  - JoeanAmier/TikTokDownloader（GPL-3.0，只读接口名和字段名）：视频的网页是 /video/<id>，图集是 /note/<id>。
 *  - Evil0ctal/Douyin_TikTok_Download_API（Apache-2.0，只读名字和形状）：同样的四个接口路径；
 *    条目的 status.is_delete 标已删除、status.private_status 不为 0 的是不公开；
 *    看不到的喜欢列表回答 aweme_list 为 null、没有 has_more，所以 aweme_list 是 null 也算认出了列表（只是空的）。
 * digg_time、collect_time 按秒记（DataReview 的示例里是秒），大于 1e11 的当作毫秒；发布时间 create_time 不是你喜欢、收藏的时间，不用。
 *
 * 交给 usuyo 的部分（握手、令牌、只连 127.0.0.1:47615）原样照 usuyo-fetch/bilibili 的
 * userscript/usuyo-bilibili-saved.user.js 0.2.0：经脚本管理器的 GM_xmlhttpRequest；先握手确认对面是 usuyo，
 * 核对不过就什么都不发，投递令牌只在核对过以后才放进请求头。
 *
 * 令牌从浏览器自己的输入对话框（prompt）或脚本管理器的菜单填，不进页面里的输入框。
 * 脚本和页面脚本可能跑在同一个环境里（例如 Violentmonkey 的页面模式），页面可以改掉
 * String、RegExp、Function 的原型方法。所以碰令牌的每一步（核对形状、取出 id、去掉首尾空白、
 * 算 HMAC 的密钥字节、显示前几位）都只用下标 t[i]、字符串拼接和脚本一开始就拿住的函数，
 * 不再按名字去原型上找方法。这只在脚本先于页面脚本跑起来时有用（@run-at document-start）；
 * 脚本管理器注入得晚、页面已经先改过这些函数时，它挡不住。令牌交给 GM_xmlhttpRequest 以后
 * 怎么走，由脚本管理器负责。
 */

(function () {
  'use strict';


  const VERSION = '0.1.0';
  const page = unsafeWindow;
  // 页面自己请求的三个列表和收藏夹的名字（只比路径；主机只认和页面同一个站）
  const LIKE_ENDPOINT = /^\/aweme\/v1\/web\/aweme\/favorite\/?$/;
  const COLLECT_ENDPOINT = /^\/aweme\/v1\/web\/aweme\/listcollection\/?$/;
  const FOLDER_ITEMS_ENDPOINT = /^\/aweme\/v1\/web\/collects\/video\/list\/?$/;
  const FOLDERS_ENDPOINT = /^\/aweme\/v1\/web\/collects\/list\/?$/;
  // 接口路径下名字像喜欢、收藏的（改了名也能进诊断）；认得的只有上面那几个
  const LIKELY = /^\/aweme\/.*(?:favorite|collect|digg|like)/i;
  const SELF_PAGE = /^\/user\/self(?:\/|$)/;
  const LABEL = { like: '喜欢', collect: '收藏' };
  // 交给 usuyo 的 action：喜欢是 like，收藏和收藏夹是 favorite
  const ACTION = { like: 'like', collect: 'favorite' };
  const AWEME_ID = /^\d{1,30}$/;
  const WORK_URL = 'https://www.douyin.com/video/';
  const NOTE_URL = 'https://www.douyin.com/note/';
  // usuyo 的本机端口。测试载入源码时把这一行换成别的端口；用户这边没有设置项。
  const PORT = 47615;
  const USUYO = 'http://127.0.0.1:' + PORT;
  const REQUEST_TIMEOUT_MS = 5000;
  // 喜欢、收藏页打开这么久还没认出一个列表回答，面板上才提一句诊断
  const QUIET_MS = 30000;
  const BATCH_MAX = 5000;
  // 投递令牌：`<16 位 id>.<64 位秘密>`，都是小写十六进制，在 usuyo 的导入页发
  const TOKEN_ID_LEN = 16;
  const TOKEN_LEN = TOKEN_ID_LEN + 1 + 64;

  // 碰到令牌的几样东西在页面脚本跑起来以前就拿住，页面之后改了原型也换不掉它们
  const reflectApply = Reflect.apply;
  const Bytes = Uint8Array;
  const subtle = crypto.subtle;
  const importKey = subtle.importKey.bind(subtle);
  const sign = subtle.sign.bind(subtle);
  const getRandomValues = crypto.getRandomValues.bind(crypto);
  const nativePrompt = page.prompt;
  // 脚本跑起来时页面的载入状态；注入得晚的话这里不是 loading
  const STARTED_AS = typeof document === 'undefined' ? 'unknown' : String(document.readyState || 'unknown');
  const clipboard = typeof navigator === 'undefined' ? null : navigator.clipboard;
  const attachShadow = Element.prototype.attachShadow;
  // 字符 → 字节，只收令牌里会出现的字符；没有原型，查表不经过任何方法
  const TOKEN_BYTE = Object.create(null);
  for (let c = 0; c < 10; c++) TOKEN_BYTE['0123456789'[c]] = 48 + c;
  for (let c = 0; c < 6; c++) TOKEN_BYTE['abcdef'[c]] = 97 + c;
  const SPACE = Object.create(null);
  for (const c of [' ', '\t', '\n', '\r', '\v', '\f', ' ', ' ', ' ', '　', '﻿']) SPACE[c] = true;

  // ---------- 记下的条目，存在脚本管理器里，页面读不到 ----------

  /**
   * like / collect：作品 id → {title, up, folder, at, note}；folder 是收藏夹 id（没有就是 ''）；
   * names：收藏夹 id → 名字。
   * @type {{like: Object<string, Item>, collect: Object<string, Item>, names: Object<string, string>}}
   */
  const store = loadStore();
  const status = { like: { hasMore: null }, collect: { hasMore: null }, warning: '', notice: '' };
  // 跳过的条目只在这一页里数（用 id 去重，页面重新请求同一页不会数两遍）
  const skipped = { other: new Set(), removed: new Set() };

  function loadStore() {
    const empty = { like: {}, collect: {}, names: {} };
    let saved;
    try {
      saved = GM_getValue('items', empty);
    } catch (e) {
      return empty;
    }
    const out = { like: {}, collect: {}, names: {} };
    for (const source of ['like', 'collect']) {
      const items = (saved && saved[source]) || {};
      for (const id of Object.keys(items)) {
        const item = items[id];
        if (AWEME_ID.test(id) && item && typeof item.title === 'string') out[source][id] = item;
      }
    }
    const names = (saved && saved.names) || {};
    for (const id of Object.keys(names)) if (AWEME_ID.test(id) && typeof names[id] === 'string') out.names[id] = names[id];
    return out;
  }

  function saveStore() {
    GM_setValue('items', store);
  }

  function count(source) {
    return Object.keys(store[source]).length;
  }

  // ---------- 诊断：只记回答的形状，不记任何值 ----------
  // 下面这一段（到「诊断结束」）在三个 usuyo-fetch 油猴脚本里逐字相同，形状归约照
  // usuyo-fetch/wechat-channels 的 userscript/usuyo-sph-probe.user.js；改一处要三处一起改。

  const LIMITS = {
    depth: 24,
    nodes: 8000,
    arrayItems: 4,
    keys: 80,
    union: 4,
    textBytes: 4000000,
    // 诊断里最多留几种不同的回答
    answers: 20,
  };

  function safeKey(key) {
    if (!/^[A-Za-z_$][A-Za-z0-9_$]{0,39}$/.test(key)) return false;
    if ((key.match(/[0-9]/g) || []).length > 3) return false;
    return !/^[0-9a-f]{12,}$/i.test(key);
  }

  function tryJson(text) {
    try {
      return { ok: true, value: JSON.parse(text) };
    } catch (_) {
      return { ok: false };
    }
  }

  function looksJson(text) {
    if (text.length < 2 || text.length > LIMITS.textBytes) return false;
    const c = text.trimStart()[0];
    return c === '{' || c === '[';
  }

  function binaryShape(v) {
    const tag = Object.prototype.toString.call(v);
    if (tag === '[object ArrayBuffer]' || tag === '[object SharedArrayBuffer]') return 'binary:' + v.byteLength;
    if (ArrayBuffer.isView(v)) return 'binary:' + v.byteLength;
    if (tag === '[object Blob]' || tag === '[object File]') return 'blob:' + v.size;
    return null;
  }

  // Reduces a value to its structure: key paths, value types and lengths. Strings become their
  // length, numbers their type. A string holding JSON is opened `jsonLeft` more levels.
  function shapeOf(v, depth, budget, jsonLeft) {
    depth = depth || 0;
    budget = budget || { n: 0 };
    jsonLeft = jsonLeft === undefined ? 1 : jsonLeft;
    if (++budget.n > LIMITS.nodes) return '…';
    if (v === null) return 'null';
    switch (typeof v) {
      case 'undefined':
      case 'boolean':
      case 'number':
      case 'bigint':
      case 'function':
      case 'symbol':
        return typeof v;
      case 'string':
        if (jsonLeft > 0 && looksJson(v)) {
          const parsed = tryJson(v);
          if (parsed.ok) return { $json: v.length, $value: shapeOf(parsed.value, depth + 1, budget, jsonLeft - 1) };
        }
        return 'string:' + v.length;
      default:
        break;
    }
    if (depth >= LIMITS.depth) return '…';
    const bin = binaryShape(v);
    if (bin) return bin;
    if (Array.isArray(v)) {
      if (v.length === 0) return { $array: 0 };
      let items;
      const n = Math.min(v.length, LIMITS.arrayItems);
      for (let i = 0; i < n; i++) {
        const s = shapeOf(v[i], depth + 1, budget, jsonLeft);
        items = i === 0 ? s : mergeShapes(items, s);
      }
      return { $array: v.length, $items: items };
    }
    const tag = Object.prototype.toString.call(v);
    if (tag === '[object Date]') return 'date';
    if (tag === '[object Map]' || tag === '[object Set]') return tag.slice(8, -1).toLowerCase() + ':' + v.size;
    const out = {};
    let other = null;
    let otherCount = 0;
    let keys;
    try {
      keys = Object.keys(v);
    } catch (_) {
      return 'object';
    }
    let taken = 0;
    for (const k of keys) {
      let child;
      try {
        child = shapeOf(v[k], depth + 1, budget, jsonLeft);
      } catch (_) {
        child = 'unreadable';
      }
      if (safeKey(k) && taken < LIMITS.keys) {
        out[k] = child;
        taken++;
      } else {
        otherCount++;
        other = other === null ? child : mergeShapes(other, child);
      }
    }
    if (otherCount) out.$otherKeys = { $count: otherCount, $value: other };
    return out;
  }

  function isRecord(s) {
    return s !== null && typeof s === 'object' && !Array.isArray(s);
  }

  function mergeRange(a, b) {
    const lo = Math.min(Array.isArray(a) ? a[0] : a, Array.isArray(b) ? b[0] : b);
    const hi = Math.max(Array.isArray(a) ? a[1] : a, Array.isArray(b) ? b[1] : b);
    return lo === hi ? lo : [lo, hi];
  }

  // Merges two shapes produced by shapeOf, so repeated answers collapse into one schema.
  function mergeShapes(a, b) {
    if (a === undefined || a === '…') return b;
    if (b === undefined || b === '…') return a;
    if (JSON.stringify(a) === JSON.stringify(b)) return a;
    if (typeof a === 'string' && typeof b === 'string') {
      const ma = /^(string|binary|blob):(\d+)(?:\.\.(\d+))?$/.exec(a);
      const mb = /^(string|binary|blob):(\d+)(?:\.\.(\d+))?$/.exec(b);
      if (ma && mb && ma[1] === mb[1]) {
        const lo = Math.min(+ma[2], +mb[2]);
        const hi = Math.max(+(ma[3] || ma[2]), +(mb[3] || mb[2]));
        return lo === hi ? ma[1] + ':' + lo : ma[1] + ':' + lo + '..' + hi;
      }
    }
    if (isRecord(a) && isRecord(b) && !('$oneOf' in a) && !('$oneOf' in b)) {
      if ('$array' in a && '$array' in b) {
        const out = { $array: mergeRange(a.$array, b.$array) };
        const items = mergeShapes(a.$items, b.$items);
        if (items !== undefined) out.$items = items;
        return out;
      }
      if ('$json' in a && '$json' in b) {
        return { $json: mergeRange(a.$json, b.$json), $value: mergeShapes(a.$value, b.$value) };
      }
      if (!('$array' in a) && !('$array' in b) && !('$json' in a) && !('$json' in b)) {
        const out = {};
        for (const k of Object.keys(a)) out[k] = k in b ? mergeShapes(a[k], b[k]) : a[k];
        for (const k of Object.keys(b)) if (!(k in a)) out[k] = b[k];
        if (a.$otherKeys && b.$otherKeys) {
          out.$otherKeys = {
            $count: Math.max(a.$otherKeys.$count, b.$otherKeys.$count),
            $value: mergeShapes(a.$otherKeys.$value, b.$otherKeys.$value),
          };
        }
        return out;
      }
    }
    const options = [];
    for (const s of [].concat(isRecord(a) && a.$oneOf ? a.$oneOf : [a], isRecord(b) && b.$oneOf ? b.$oneOf : [b])) {
      if (!options.some((o) => JSON.stringify(o) === JSON.stringify(s))) options.push(s);
    }
    return { $oneOf: options.length > LIMITS.union ? options.slice(0, LIMITS.union).concat('…') : options };
  }

  // ---------- paths ----------

  function safeSegment(seg) {
    if (!/^[A-Za-z0-9_.-]{1,40}$/.test(seg)) return ':x';
    if ((seg.match(/[0-9]/g) || []).length > 6) return ':x';
    if (/^[0-9a-f]{16,}$/i.test(seg)) return ':x';
    return seg;
  }

  // Pathname only, with id-like segments replaced; query and hash are dropped.
  function safePath(pathname) {
    return pathname
      .split('/')
      .map((seg, i) => (i === 0 || seg === '' ? seg : safeSegment(seg)))
      .join('/');
  }

  // 请求地址的路径：去掉协议、主机、查询和片段
  function pathOf(rawUrl) {
    const path = String(rawUrl).replace(/^(?:[a-z][a-z0-9+.-]*:)?\/\/[^/?#]*/i, '').split(/[?#]/)[0];
    return path.charAt(0) === '/' ? path : '/' + path;
  }

  // 诊断里的路径：像 id 的段（带数字、或长得不像一个词）换成 :x
  function diagPath(rawUrl) {
    return safePath(pathOf(rawUrl))
      .split('/')
      .map((seg) => (seg.length > 30 || (/[0-9]/.test(seg) && !/^v[0-9]{1,2}$/.test(seg)) ? ':x' : seg))
      .join('/')
      .slice(0, 120);
  }

  // 脚本数过的东西只用几个固定的词；别的词一律记成 other
  function safeWord(word) {
    return typeof word === 'string' && /^[a-z][A-Za-z]{0,24}$/.test(word) ? word : 'other';
  }

  function whole(n) {
    return Number.isSafeInteger(n) && n > 0 ? n : 0;
  }

  /** 一个回答的文本：能解析成 JSON 就带上它的形状，否则只记长度。 */
  function readAnswer(text) {
    const parsed = tryJson(text);
    if (parsed.ok) return { json: true, value: parsed.value, shape: shapeOf(parsed.value) };
    return { json: false, value: undefined, shape: 'string:' + String(text).length };
  }

  /** 没认出是列表回答时的结果；认出时 `recognised` 为 true，`skipped` 是按固定的词分的跳过数。 */
  const NOT_RECOGNISED = Object.freeze({ recognised: false, took: 0, skipped: Object.freeze({}), stopped: '' });

  const answerLog = new Map();
  // requests：经过脚本的 fetch、XHR 一共多少个（只是个数），用来分清「没看到请求」和「看到了但不认得」
  const answerStats = { requests: 0, looked: 0, recognised: 0, dropped: 0 };

  /** 记一个看过的回答；同一路径、状态、是否 JSON、是否认出的重复合并，只留最近 LIMITS.answers 种。 */
  function logAnswer(path, httpStatus, json, shape, outcome) {
    answerStats.looked++;
    if (outcome.recognised) answerStats.recognised++;
    const code = whole(httpStatus);
    const key = [path, code, json, outcome.recognised].join(' ');
    const entry = answerLog.get(key) || { path, status: code, json, recognised: outcome.recognised, answers: 0, took: 0, skipped: {}, stopped: [], shape: undefined };
    answerLog.delete(key);
    entry.answers++;
    entry.took += whole(outcome.took);
    for (const kind of Object.keys(outcome.skipped)) {
      const word = safeWord(kind);
      entry.skipped[word] = (entry.skipped[word] || 0) + whole(outcome.skipped[kind]);
    }
    if (outcome.stopped) {
      const word = safeWord(outcome.stopped);
      if (!entry.stopped.includes(word)) entry.stopped.push(word);
    }
    entry.shape = mergeShapes(entry.shape, shape);
    answerLog.set(key, entry);
    if (answerLog.size > LIMITS.answers) {
      answerLog.delete(answerLog.keys().next().value);
      answerStats.dropped++;
    }
  }

  function answerReport() {
    return {
      requests: answerStats.requests,
      looked: answerStats.looked,
      recognised: answerStats.recognised,
      dropped: answerStats.dropped,
      answers: Array.from(answerLog.values(), (e) => ({
        path: e.path,
        status: e.status,
        json: e.json,
        recognised: e.recognised,
        answers: e.answers,
        took: e.took,
        skipped: Object.assign({}, e.skipped),
        stopped: e.stopped.slice(),
        shape: e.shape,
      })),
    };
  }

  // ---------- 诊断结束 ----------

  // ---------- 只看页面自己请求回来的列表 ----------

  /** 请求地址的主机是页面自己的站（抖音的接口在 www 或别的子域上）。 */
  function sameSite(hostname) {
    const mine = String(page.location.hostname || '').split('.').slice(-2).join('.');
    return hostname === mine || hostname.endsWith('.' + mine);
  }

  function parsedUrl(rawUrl) {
    try {
      return new URL(String(rawUrl), page.location.href);
    } catch (e) {
      return null;
    }
  }

  /** 要读的回答：页面自己站上接口路径下的；别的主机的回答不读。 */
  function looked(rawUrl) {
    const u = parsedUrl(rawUrl);
    return Boolean(u) && sameSite(u.hostname) && /^\/aweme\//.test(u.pathname);
  }

  /** 秒级时间戳 → 毫秒（已经是毫秒的照用）；不是正数就是 0（不知道）。 */
  function millis(n) {
    if (typeof n !== 'number' || !Number.isFinite(n) || n <= 0) return 0;
    return Math.round(n > 1e11 ? n : n * 1000);
  }

  function idOf(v) {
    if (typeof v === 'string') return AWEME_ID.test(v) ? v : '';
    if (typeof v === 'number' && Number.isSafeInteger(v) && v >= 0) return String(v);
    return '';
  }

  /** 回答里的列表在最外层，或者（示例里）包在 data 里。 */
  function bodyOf(json) {
    if ('aweme_list' in json || 'collects_list' in json) return json;
    return json.data && typeof json.data === 'object' && !Array.isArray(json.data) ? json.data : json;
  }

  /** 列表在不在：数组，或者 null（看不到的列表回的就是 null）；字段没有就是没认出。 */
  function listOf(body, field) {
    if (Array.isArray(body[field])) return body[field];
    return body[field] === null ? [] : null;
  }

  /** 条目被标成已删除，或不公开（private_status 不为 0）。 */
  function isRemoved(m) {
    const s = m.status && typeof m.status === 'object' ? m.status : {};
    return s.is_delete === true || (typeof s.private_status === 'number' && s.private_status !== 0);
  }

  function firstLine(text) {
    const s = typeof text === 'string' ? text : '';
    const end = s.search(/[\r\n]/);
    return (end < 0 ? s : s.slice(0, end)).trim().slice(0, 200);
  }

  /** 一页喜欢、收藏或收藏夹里作品；回 {recognised, took, skipped}。folder 为 null 是喜欢或整个收藏。 */
  function absorbWorks(source, folder, body) {
    const entries = listOf(body, 'aweme_list');
    if (entries === null) return { recognised: false, took: 0, skipped: {}, stopped: '' };
    if ('has_more' in body && folder === null) status[source].hasMore = [1, true, '1'].includes(body.has_more);
    const timeField = source === 'like' ? 'digg_time' : 'collect_time';
    let took = 0;
    const gone = { badId: 0, removed: 0 };
    for (const m of entries) {
      const id = m && typeof m === 'object' ? idOf(m.aweme_id) : '';
      if (!id) {
        // 没有 id 的条目没有稳定的名字，用标题和作者当键，页面重新请求同一页不会数两遍
        const text = m && typeof m === 'object' ? firstLine(m.desc) + '|' + String((m.author && m.author.nickname) || '') : '';
        skipped.other.add('?:' + text);
        gone.badId++;
        continue;
      }
      if (isRemoved(m)) {
        skipped.removed.add(id);
        gone.removed++;
        continue;
      }
      const author = m.author && typeof m.author === 'object' ? m.author : {};
      const before = store[source][id];
      store[source][id] = {
        title: firstLine(m.desc),
        up: typeof author.nickname === 'string' ? author.nickname.slice(0, 100) : '',
        folder: folder || (before && before.folder) || '',
        at: millis(m[timeField]) || (before && before.at) || 0,
        note: Array.isArray(m.images) && m.images.length > 0,
      };
      took++;
    }
    return { recognised: true, took, skipped: gone, stopped: '' };
  }

  function absorbFolders(body) {
    const entries = listOf(body, 'collects_list');
    if (entries === null) return { recognised: false, took: 0, skipped: {}, stopped: '' };
    let took = 0;
    for (const f of entries) {
      if (!f || typeof f !== 'object') continue;
      const id = idOf(f.collects_id_str) || idOf(f.collects_id);
      if (!id || typeof f.collects_name !== 'string' || !f.collects_name.trim()) continue;
      store.names[id] = f.collects_name.trim().slice(0, 100);
      took++;
    }
    return { recognised: true, took, skipped: { badId: entries.length - took }, stopped: '' };
  }

  /** 认得是哪个接口就记下，回一份「认出了吗、收了几条、按什么跳过」给诊断。 */
  function absorb(u, json) {
    const path = u.pathname;
    const kind = LIKE_ENDPOINT.test(path) ? 'like' : COLLECT_ENDPOINT.test(path) ? 'collect' : FOLDER_ITEMS_ENDPOINT.test(path) ? 'folder-items' : FOLDERS_ENDPOINT.test(path) ? 'folders' : '';
    if (!kind || !json || typeof json !== 'object' || Array.isArray(json)) return NOT_RECOGNISED;
    const code = json.status_code;
    if (code !== undefined && code !== null && String(code) !== '0') {
      // 掉线或风控时页面自己拿到的就是这些码；照实说出来，让你停手
      status.warning = '抖音回了 status_code ' + String(code).slice(0, 12) + '，先别往下翻了';
      render();
      return { recognised: false, took: 0, skipped: {}, stopped: 'errorCode' };
    }
    const body = bodyOf(json);
    const outcome =
      kind === 'folders'
        ? absorbFolders(body)
        : kind === 'folder-items'
          ? absorbWorks('collect', idOf(u.searchParams.get('collects_id')), body)
          : absorbWorks(kind, null, body);
    if (outcome.recognised) {
      status.notice = '';
      status.warning = '';
      saveStore();
      render();
    }
    return outcome;
  }

  /** 回答里有没有一个列表数组：不认得的路径上也有，诊断要留住它。 */
  function carriesList(value) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
    const body = bodyOf(value);
    return Array.isArray(body.aweme_list) || Array.isArray(body.collects_list);
  }

  /** 看过的回答：认得的交给 absorb；认不得的，名字像的、或带着列表数组的，记下形状，别的不留。 */
  function take(rawUrl, httpStatus, answer) {
    const u = parsedUrl(rawUrl);
    if (!u) return;
    let outcome = NOT_RECOGNISED;
    if (answer.json) {
      try {
        outcome = absorb(u, answer.value);
      } catch (e) {
        outcome = { recognised: false, took: 0, skipped: {}, stopped: 'failed' };
      }
    }
    if (outcome.recognised || outcome.stopped || LIKELY.test(u.pathname) || (answer.json && carriesList(answer.value))) {
      logAnswer(diagPath(u.pathname), httpStatus, answer.json, answer.shape, outcome);
    }
  }

  // fetch：拿响应的副本读，原响应原样交回页面
  const originalFetch = page.fetch;
  page.fetch = function (input) {
    const result = originalFetch.apply(this, arguments);
    try {
      const url = typeof input === 'string' ? input : (input && (input.url || input.href)) || '';
      if (looked(url)) {
        result
          .then((r) => r.clone().text().then((text) => take(url, r.status, readAnswer(text))))
          .catch(() => {});
      }
    } catch (e) {
      // 读不了就不记，页面照常
    }
    return result;
  };

  // XHR：请求地址记在脚本自己的 WeakMap 里，不往页面的对象上挂属性
  const xhrUrls = new WeakMap();
  const xhr = page.XMLHttpRequest.prototype;
  const originalOpen = xhr.open;
  const originalSend = xhr.send;
  xhr.open = function (_method, url) {
    try {
      xhrUrls.set(this, String(url));
    } catch (e) {
      // 同上
    }
    return originalOpen.apply(this, arguments);
  };
  xhr.send = function () {
    const url = xhrUrls.get(this) || '';
    if (looked(url)) {
      this.addEventListener('load', () => {
        try {
          const answer =
            this.responseType === 'json'
              ? { json: this.response !== null, value: this.response, shape: shapeOf(this.response) }
              : readAnswer(this.responseText);
          take(this.responseURL || url, this.status, answer);
        } catch (e) {
          // 同上
        }
      });
    }
    return originalSend.apply(this, arguments);
  };

  // ---------- 交给 usuyo ----------

  const UNREACHABLE = new Error('unreachable');
  let busy = false;

  // ---------- 令牌：下面几个函数只用 t[i]、length 和拼接 ----------

  function isToken(t) {
    if (typeof t !== 'string' || t.length !== TOKEN_LEN) return false;
    for (let i = 0; i < TOKEN_LEN; i++) {
      if (i === TOKEN_ID_LEN ? t[i] !== '.' : TOKEN_BYTE[t[i]] === undefined) return false;
    }
    return true;
  }

  /** 令牌 `.` 前的 id（不是秘密）的前 `n` 位。 */
  function tokenId(t, n) {
    let id = '';
    for (let i = 0; i < n; i++) id += t[i];
    return id;
  }

  /** 令牌的字节（HMAC 的密钥）；令牌只有 ASCII，一个字符一个字节。 */
  function tokenBytes(t) {
    const b = new Bytes(t.length);
    for (let i = 0; i < t.length; i++) b[i] = t[i] === '.' ? 46 : TOKEN_BYTE[t[i]];
    return b;
  }

  /** 去掉首尾空白，不用 String.prototype.trim。 */
  function trimmed(v) {
    let start = 0;
    let end = v.length;
    while (start < end && SPACE[v[start]]) start++;
    while (end > start && SPACE[v[end - 1]]) end--;
    let out = '';
    for (let i = start; i < end; i++) out += v[i];
    return out;
  }

  function storedToken() {
    const t = GM_getValue('deliverToken', '');
    return isToken(t) ? t : '';
  }

  /** 只连本机的 usuyo；连不上、超时都当作没人在听。 */
  function local(method, path, headers, data) {
    return new Promise((resolve, reject) => {
      GM_xmlhttpRequest({
        method,
        url: USUYO + path,
        headers,
        data,
        timeout: REQUEST_TIMEOUT_MS,
        anonymous: true,
        onload: (r) => resolve({ status: r.status, text: String(r.responseText || '') }),
        onerror: () => reject(UNREACHABLE),
        ontimeout: () => reject(UNREACHABLE),
        onabort: () => reject(UNREACHABLE),
      });
    });
  }

  function hex(bytes) {
    return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
  }

  /** 和 usuyo 的 crates/usuyo-api/src/handshake.rs 签同样的字节。 */
  async function proofOf(token, nonce) {
    const enc = new TextEncoder();
    const key = await importKey('raw', tokenBytes(token), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
    const msg = 'usuyo-daemon-proof/v1\n127.0.0.1:' + PORT + '\n' + nonce;
    return hex(new Uint8Array(await sign('HMAC', key, enc.encode(msg))));
  }

  function same(a, b) {
    if (a.length !== b.length) return false;
    let diff = 0;
    for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
    return diff === 0;
  }

  /**
   * 握手：只发一个新的随机数和令牌的 id（`.` 前那段，不是秘密），对面用整个令牌签回来，
   * 这边用自己存着的令牌核对。回 'usuyo'、'refused'（usuyo 不让这个来源进）或 'stranger'。
   */
  async function prove(token) {
    const nonce = hex(getRandomValues(new Uint8Array(32)));
    const client = tokenId(token, TOKEN_ID_LEN);
    const res = await local('POST', '/handshake', { 'Content-Type': 'application/json' }, JSON.stringify({ nonce, client }));
    if (res.status === 403) return 'refused';
    if (res.status !== 200) return 'stranger';
    let proof;
    try {
      proof = JSON.parse(res.text).proof;
    } catch (e) {
      return 'stranger';
    }
    if (typeof proof !== 'string' || !/^[0-9a-f]{64}$/.test(proof)) return 'stranger';
    return same(await proofOf(token, nonce), proof) ? 'usuyo' : 'stranger';
  }

  function today() {
    const d = new Date();
    return d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0') + '-' + String(d.getDate()).padStart(2, '0');
  }

  function workUrl(id, item) {
    return (item.note ? NOTE_URL : WORK_URL) + id;
  }

  /** 记下的条目：喜欢一份，收藏按收藏夹的名字分开（没有名字的归在「收藏」里），每批不超过 BATCH_MAX。 */
  function batches() {
    const out = [];
    for (const source of ['like', 'collect']) {
      const groups = new Map();
      for (const id of Object.keys(store[source])) {
        const folder = source === 'collect' ? store.names[store[source][id].folder] || '' : '';
        if (!groups.has(folder)) groups.set(folder, []);
        groups.get(folder).push(id);
      }
      for (const [folder, ids] of groups) {
        for (let i = 0; i < ids.length; i += BATCH_MAX) out.push({ source, folder, ids: ids.slice(i, i + BATCH_MAX) });
      }
    }
    return out;
  }

  function batchName(b) {
    return '抖音' + LABEL[b.source] + (b.folder ? '「' + b.folder + '」' : '') + ' ' + today();
  }

  async function deliver() {
    if (busy) return;
    const todo = batches();
    if (!todo.length) {
      status.notice = '还没有记下的条目：先在喜欢、收藏页往下翻';
      render();
      return;
    }
    let token = storedToken();
    if (!token) {
      if (!askToken()) return;
      token = storedToken();
    }
    busy = true;
    status.warning = '';
    status.notice = '先确认本机上的是 usuyo…';
    render();
    const handed = { like: 0, collect: 0 };
    let stop = '';
    try {
      const who = await prove(token);
      if (who === 'refused') {
        stop = 'usuyo 没让这个脚本管理器的请求进来，什么都没交';
      } else if (who === 'stranger') {
        stop = '端口 ' + PORT + ' 上的程序没能证明自己是 usuyo（也可能这个令牌在 usuyo 里撤掉了），令牌没有发出去，什么都没交';
      } else {
        for (const b of todo) {
          const links = b.ids.map((id) => {
            const item = store[b.source][id];
            const link = { url: workUrl(id, item) };
            if (item.title) link.title = item.title;
            if (item.at) link.actedAt = new Date(item.at).toISOString();
            return link;
          });
          const res = await local(
            'POST',
            '/v1/batches',
            { Authorization: 'Bearer ' + token, 'Content-Type': 'application/json' },
            JSON.stringify({ name: batchName(b), action: ACTION[b.source], links }),
          );
          if (res.status === 401) {
            stop = 'usuyo 不认这个令牌（可能撤掉了），剩下的没有收下，还存着；点「忘掉令牌」，再贴一个新的';
            break;
          }
          if (res.status !== 202) {
            stop = 'usuyo 这一批没有收下（回了 ' + res.status + '），剩下的还存着';
            break;
          }
          for (const id of b.ids) delete store[b.source][id];
          saveStore();
          handed[b.source] += b.ids.length;
        }
      }
    } catch (e) {
      if (e !== UNREACHABLE) throw e;
      stop = 'usuyo 没有开着（本机 ' + PORT + ' 端口没有回应），' + (handed.like + handed.collect ? '剩下的还存着' : '什么都没交');
    } finally {
      busy = false;
    }
    const total = handed.like + handed.collect;
    const done = total ? '交给 usuyo 了：喜欢 ' + handed.like + ' 条、收藏 ' + handed.collect + ' 条，到 usuyo 的导入页确认了才会去取。' : '';
    status.notice = done + stop;
    render();
  }

  /** 用浏览器自己的对话框问令牌，存进脚本管理器；存下了回 true。 */
  function askToken() {
    let value = null;
    try {
      value = reflectApply(nativePrompt, page, ['贴上 usuyo 导入页发的令牌（交进来的清单 › 发一个令牌）。它只存在脚本管理器里。']);
    } catch (e) {
      value = null;
    }
    if (typeof value !== 'string' || !trimmed(value).length) {
      status.notice = '没拿到令牌（对话框被关掉或没弹出来）；也可以从脚本管理器的菜单里点「填 usuyo 令牌」';
      render();
      return false;
    }
    value = trimmed(value);
    if (!isToken(value)) {
      status.notice = '这不像 usuyo 的投递令牌：应是一串 id、一个点、再一串秘密';
      render();
      return false;
    }
    GM_setValue('deliverToken', value);
    status.notice = '令牌存着了';
    render();
    return true;
  }

  // 按钮的入口：意料之外的错误在这里说出来，不留在「先确认…」上
  function handOver() {
    deliver().catch((e) => {
      status.notice = '没交成：' + String((e && e.message) || e).slice(0, 80);
      render();
    });
  }

  function forgetToken() {
    GM_setValue('deliverToken', '');
    status.notice = '令牌忘掉了；下次交的时候再贴一个';
    render();
  }

  GM_registerMenuCommand('填 usuyo 令牌', askToken);
  GM_registerMenuCommand('忘掉 usuyo 令牌', forgetToken);

  function clearAll() {
    store.like = {};
    store.collect = {};
    store.names = {};
    skipped.other.clear();
    skipped.removed.clear();
    status.notice = '';
    status.warning = '';
    saveStore();
    render();
  }

  // ---------- 诊断：交给你复制，脚本自己不往任何地方发 ----------

  const QUIET_NOTE = '这一页打开一会儿了，还没认出喜欢或收藏的回答。点「复制诊断」，把它贴给我们就行。';
  const DIAG_NOTE = '诊断里只有回答的结构，没有任何内容。';

  /** 当前页的种类，只用固定的两个词，不带账号 id。 */
  function pageKind() {
    return SELF_PAGE.test(page.location.pathname) ? 'self' : 'other';
  }

  function diagnosis() {
    return Object.assign({ tool: 'usuyo-douyin-saved', version: VERSION, page: pageKind(), startedAs: STARTED_AS }, answerReport());
  }

  function copyDiagnosis() {
    const text = JSON.stringify(diagnosis(), null, 2);
    const refused = () => {
      ui.paste.value = text;
      ui.paste.style.display = 'block';
      ui.paste.select();
      status.notice = '没能写进剪贴板：下面的框里是同样的文字，请手动复制。';
      render();
    };
    try {
      const written = clipboard && clipboard.writeText(text);
      if (!written || typeof written.then !== 'function') {
        refused();
        return;
      }
      written.then(() => {
        ui.paste.style.display = 'none';
        status.notice = '诊断已复制，只含结构。';
        render();
      }, refused);
    } catch (e) {
      refused();
    }
  }

  // ---------- 角落里的一小块计数 ----------

  let ui = null;
  // 喜欢、收藏页是什么时候开始看的；离开就清零
  let savedSince = 0;

  function mount() {
    if (ui || !document.body) return;
    const host = document.createElement('div');
    const root = attachShadow.call(host, { mode: 'closed' });
    const style = document.createElement('style');
    style.textContent =
      ':host{all:initial}' +
      '.box{position:fixed;right:12px;bottom:12px;z-index:2147483647;max-width:260px;padding:8px 10px;' +
      'border-radius:8px;background:rgba(32,33,36,.86);color:#eee;font:12px/1.5 -apple-system,system-ui,sans-serif}' +
      '.row{display:flex;gap:6px;margin-top:6px}' +
      'button{flex:1;padding:3px 6px;border:1px solid rgba(255,255,255,.25);border-radius:5px;' +
      'background:transparent;color:inherit;font:inherit;cursor:pointer}' +
      'button:hover{background:rgba(255,255,255,.12)}' +
      '.key{display:flex;gap:6px;align-items:center;margin-top:4px;opacity:.75}.key button{flex:0 0 auto}' +
      '.skip,.note{margin-top:4px;opacity:.75}.warn{margin-top:4px;color:#ffb4a8}' +
      '.diag{display:flex;gap:6px;align-items:baseline;margin-top:6px;opacity:.6;font-size:11px}' +
      '.diag button{flex:0 0 auto;padding:0;border:0;text-decoration:underline}';
    const box = document.createElement('div');
    box.className = 'box';
    const line = document.createElement('div');
    const skip = document.createElement('div');
    skip.className = 'skip';
    const warn = document.createElement('div');
    warn.className = 'warn';
    const note = document.createElement('div');
    note.className = 'note';
    const row = document.createElement('div');
    row.className = 'row';
    const go = document.createElement('button');
    go.textContent = '交给 usuyo';
    go.addEventListener('click', handOver);
    const clear = document.createElement('button');
    clear.textContent = '清空';
    clear.addEventListener('click', clearAll);
    row.append(go, clear);
    // 令牌存着时只显示 id 的前几位
    const key = document.createElement('div');
    key.className = 'key';
    const keyText = document.createElement('span');
    const forget = document.createElement('button');
    forget.textContent = '忘掉令牌';
    forget.addEventListener('click', forgetToken);
    key.append(keyText, forget);
    // 诊断：不显眼的次要动作，旁边一句话说清里面只有结构
    const diag = document.createElement('div');
    diag.className = 'diag';
    const copy = document.createElement('button');
    copy.textContent = '复制诊断';
    copy.addEventListener('click', copyDiagnosis);
    const diagText = document.createElement('span');
    diagText.textContent = DIAG_NOTE;
    diag.append(copy, diagText);
    const paste = document.createElement('textarea');
    paste.readOnly = true;
    Object.assign(paste.style, { display: 'none', width: '100%', height: '80px', marginTop: '4px', boxSizing: 'border-box', font: '11px monospace' });
    box.append(line, skip, warn, note, key, row, diag, paste);
    root.append(style, box);
    document.body.appendChild(host);
    ui = { line, skip, warn, note, key, keyText, paste };
    render();
  }

  function render() {
    if (!ui) return;
    const part = (source) => {
      const end = status[source].hasMore === false ? '（已到底）' : '';
      return LABEL[source] + ' ' + count(source) + end;
    };
    ui.line.textContent = 'usuyo · ' + part('like') + ' · ' + part('collect');
    const other = skipped.other.size;
    const removed = skipped.removed.size;
    ui.skip.textContent = other || removed ? '跳过：缺 id 的 ' + other + ' 条、已删除或不公开的 ' + removed + ' 条' : '';
    ui.warn.textContent = status.warning;
    const onSaved = pageKind() === 'self';
    if (!onSaved) savedSince = 0;
    else if (!savedSince) savedSince = Date.now();
    const quiet = onSaved && Date.now() - savedSince > QUIET_MS && answerStats.recognised === 0;
    ui.note.textContent = status.notice || (quiet ? QUIET_NOTE : '自己往下翻，翻到的会记下来');
    const token = storedToken();
    ui.key.style.display = token ? '' : 'none';
    ui.keyText.textContent = token ? '令牌 ' + tokenId(token, 6) + '… 存在脚本管理器里' : '';
  }

  // 脚本先于页面载入，等 body 出来再挂上面板；每秒刷一次，让「打开了多久」的提示到点出现
  setInterval(() => {
    mount();
    render();
  }, 1000);
})();
