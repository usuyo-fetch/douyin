// 离线载入油猴脚本：假的 GM_*、unsafeWindow、document，页面的列表请求打到本机的假站点。
// 页面请求写的是 https://www.douyin.com/…，harness 在真正发出前把主机换成假站点，脚本看到的还是原来的地址。
// 脚本的 GM_xmlhttpRequest 只放行 127.0.0.1，别的主机直接记成违规。
import { readFileSync } from 'node:fs';
import { createServer, request } from 'node:http';
import { webcrypto } from 'node:crypto';

// 脚本经下面 load() 里的 new Function 跑，栈里它的帧都写着这一段
export const SCRIPT_FRAME = 'eval at load (';
export const SCRIPT_PATH = new URL('../usuyo-douyin-saved.user.js', import.meta.url);
const SOURCE = readFileSync(SCRIPT_PATH, 'utf8');

// ---------- 一点点 DOM ----------

class Element {
  constructor(tag) {
    this.tagName = tag;
    this.children = [];
    this.style = {};
    this.listeners = {};
    this.text = '';
  }
  get textContent() {
    return this.text + this.children.map((c) => c.textContent).join(' ');
  }
  set textContent(v) {
    this.text = String(v);
    this.children = [];
  }
  append(...nodes) {
    this.children.push(...nodes);
  }
  appendChild(node) {
    this.children.push(node);
    return node;
  }
  addEventListener(type, fn) {
    (this.listeners[type] ||= []).push(fn);
  }
  click() {
    for (const fn of this.listeners.click || []) fn({ type: 'click' });
  }
  find(pred) {
    if (pred(this)) return this;
    for (const c of this.children) {
      const hit = c.find(pred);
      if (hit) return hit;
    }
    return null;
  }
  select() {
    this.selected = true;
  }
  attachShadow() {
    this.shadow = new Element('#shadow-root');
    return this.shadow;
  }
}

// ---------- 假的抖音：只答喜欢、收藏、收藏夹的接口，回答是 fixtures/ 里手写的 ----------

const fixture = (name) => readFileSync(new URL('./fixtures/' + name, import.meta.url), 'utf8');

const ANSWERS = [
  [/^\/aweme\/v1\/web\/aweme\/favorite\/$/, (q) => (q.get('private') ? 'like-private.json' : q.get('renamed') ? 'like-renamed.json' : q.get('max_cursor') === '1730000100000' ? 'like-page2.json' : 'like-page1.json')],
  [/^\/aweme\/v1\/web\/mystery\/saved\/$/, () => 'unknown-list.json'],
  [/^\/aweme\/v1\/web\/aweme\/listcollection\/$/, (q) => (q.get('odd') ? 'odd-entries.json' : 'collect.json')],
  [/^\/aweme\/v1\/web\/collects\/list\/$/, () => 'folders.json'],
  [/^\/aweme\/v1\/web\/collects\/video\/list\/$/, (q) => (q.get('collects_id') === '666' ? 'error.json' : q.get('collects_id') === '7100000000000000002' ? 'folder-items-2.json' : 'folder-items-1.json')],
  [/^\/aweme\/v1\/web\/tab\/feed\/$/, () => 'other-answer.json'],
  [/^\/aweme\/v1\/web\/aweme\/post\/$/, () => 'error.json'],
];

export async function startSite() {
  const seen = [];
  const server = createServer((req, res) => {
    seen.push(req.url);
    const url = new URL(req.url, 'http://x');
    const hit = ANSWERS.find(([re]) => re.test(url.pathname));
    if (url.pathname.endsWith('/not-json')) {
      res.setHeader('content-type', 'text/html');
      res.end('<html>verify</html>');
      return;
    }
    res.setHeader('content-type', 'application/json');
    if (!hit) {
      res.writeHead(404);
      res.end('{}');
      return;
    }
    res.end(fixture(hit[1](url.searchParams)));
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  return { origin: 'http://127.0.0.1:' + server.address().port, seen, close: () => new Promise((r) => server.close(r)) };
}

// ---------- 假的本机监听者（不是 usuyo，或者假装认得令牌） ----------

export async function startListener(answer) {
  const seen = [];
  const server = createServer((req, res) => {
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', () => {
      seen.push({ method: req.method, url: req.url, headers: req.headers, body });
      const [status, json] = answer(req, body);
      res.writeHead(status, { 'content-type': 'application/json' });
      res.end(JSON.stringify(json));
    });
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  return { port: server.address().port, seen, close: () => new Promise((r) => server.close(r)) };
}

// ---------- 载入脚本 ----------

/** 页面写的地址（任何抖音的主机）换成假站点上的同一路径。 */
function toSite(site, input) {
  const u = new URL(String(input), 'https://www.douyin.com/');
  return site + u.pathname + u.search;
}

export function load({ port, site, path = '/user/self', host = 'www.douyin.com', origin, clipboard = 'ok', fetchImpl }) {
  const marker = 'const PORT = 47615;';
  if (!SOURCE.includes(marker)) throw new Error('port line not found in the script');
  const source = SOURCE.replace(marker, 'const PORT = ' + port + ';');

  const gm = new Map();
  const gmRequests = [];
  const violations = [];
  const pageStorage = new Map();
  let tick = () => {};
  const menu = {};
  // 浏览器自己的 prompt：测试排好下一次回什么；脚本载入时会拿住这一个
  const promptAnswers = [];
  const prompted = [];

  class FakeXHR {
    constructor() {
      this.listeners = {};
      this.responseType = '';
      this.responseText = '';
      this.response = null;
      this.status = 0;
      this.responseURL = '';
    }
    addEventListener(type, fn) {
      (this.listeners[type] ||= []).push(fn);
    }
    open(method, url) {
      this.method = method;
      this.url = url;
    }
    send() {
      fetch(toSite(site, this.url), { method: this.method })
        .then(async (r) => {
          this.status = r.status;
          this.responseText = await r.text();
          if (this.responseType === 'json') this.response = JSON.parse(this.responseText);
          this.responseURL = new URL(String(this.url), 'https://www.douyin.com/').href;
          for (const fn of this.listeners.load || []) fn.call(this, {});
        })
        .catch(() => {});
    }
  }

  const page = {
    location: { href: 'https://' + host + path, pathname: path, hostname: host },
    localStorage: {
      getItem: (k) => (pageStorage.has(k) ? pageStorage.get(k) : null),
      setItem: (k, v) => pageStorage.set(k, String(v)),
    },
    prompt: (message) => {
      prompted.push(message);
      return promptAnswers.length ? promptAnswers.shift() : null;
    },
    fetch: fetchImpl || ((input, init) => fetch(toSite(site, typeof input === 'string' ? input : input.url), init)),
    XMLHttpRequest: FakeXHR,
  };

  const body = new Element('body');
  const document = {
    readyState: 'loading',
    body,
    createElement: (tag) => new Element(tag),
  };

  function GM_xmlhttpRequest(o) {
    const url = new URL(o.url);
    gmRequests.push({ method: o.method, url: o.url, headers: { ...o.headers }, data: o.data });
    if (url.hostname !== '127.0.0.1') {
      violations.push(o.url);
      queueMicrotask(() => o.onerror && o.onerror({}));
      return;
    }
    // 脚本管理器自己加的 Origin（脚本设不了它）
    const headers = origin ? { ...o.headers, Origin: origin } : o.headers;
    const req = request(
      { host: '127.0.0.1', port: url.port, path: url.pathname + url.search, method: o.method, headers, timeout: o.timeout },
      (res) => {
        let text = '';
        res.setEncoding('utf8');
        res.on('data', (c) => (text += c));
        res.on('end', () => o.onload({ status: res.statusCode, responseText: text }));
      },
    );
    req.on('timeout', () => {
      req.destroy();
      o.ontimeout();
    });
    req.on('error', () => o.onerror({}));
    if (o.data != null) req.write(o.data);
    req.end();
  }

  // 剪贴板：'ok' 收下，'refuse' 拒绝，'none' 没有这个接口
  const copied = [];
  const navigator = {
    clipboard:
      clipboard === 'none'
        ? undefined
        : { writeText: (text) => (clipboard === 'refuse' ? Promise.reject(new Error('denied')) : (copied.push(text), Promise.resolve())) },
  };

  const names = ['navigator', 'unsafeWindow', 'document', 'GM_getValue', 'GM_setValue', 'GM_xmlhttpRequest', 'GM_registerMenuCommand', 'Element', 'crypto', 'setInterval'];
  const run = new Function(...names, source);
  run(
    navigator,
    page,
    document,
    (k, d) => (gm.has(k) ? structuredClone(gm.get(k)) : d),
    (k, v) => gm.set(k, structuredClone(v)),
    GM_xmlhttpRequest,
    (label, fn) => {
      menu[label] = fn;
    },
    Element,
    webcrypto,
    (fn) => {
      tick = fn;
      return 0;
    },
  );
  tick();

  const panel = () => body.children[0].shadow;
  const button = (label) => panel().find((e) => e.tagName === 'button' && e.text === label);
  const classText = (name) => panel().find((e) => e.text !== undefined && new RegExp(name).test(e.className || '')).text;
  return {
    page,
    gm,
    gmRequests,
    violations,
    pageStorage,
    copied,
    paste: () => panel().find((e) => e.tagName === 'textarea'),
    tick,
    panelText: () => panel().textContent,
    note: () => classText('note'),
    warnText: () => panel().find((e) => e.className === 'warn').text,
    click: (label) => button(label).click(),
    answerPrompt: (value) => promptAnswers.push(value),
    prompted,
    menu: (label) => menu[label](),
    menuLabels: () => Object.keys(menu),
    hasInput: () => !!body.find((e) => e.tagName === 'input' || (e.shadow && e.shadow.find((x) => x.tagName === 'input'))),
  };
}

/** 等到 `cond()` 为真，最多 `ms` 毫秒。 */
export async function until(cond, ms = 8000) {
  const end = Date.now() + ms;
  while (!cond()) {
    if (Date.now() > end) throw new Error('timed out waiting');
    await new Promise((r) => setTimeout(r, 20));
  }
}
