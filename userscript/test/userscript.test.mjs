// node --test userscript/test/userscript.test.mjs
// 交给 usuyo 的测试要一个一次性的 usuyo 守护进程：USUYO_BIN 指向已经编好的 usuyo，
// 端口 USUYO_TEST_PORT（默认 47713，不用 47615），家目录是新建的临时目录，测完 SIGTERM 并送进废纸篓。
// 回答的形状在 fixtures/：都是照公开项目里的字段名手写的，内容是编的，没有一份量过（每份的 _note 写了照哪里）。
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, existsSync } from 'node:fs';
import { createHmac, randomBytes } from 'node:crypto';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { load, startSite, startListener, until, SCRIPT_FRAME, SCRIPT_PATH } from './harness.mjs';

const BIN = process.env.USUYO_BIN;
const PORT = Number(process.env.USUYO_TEST_PORT || 47713);
const ID = (n) => String(7300000000000000000n + BigInt(n));
const LIKES = [1, 2, 4, 5].map(ID);
const PLAIN_COLLECT = [1, 11].map(ID);
const FOLDER_A = [12, 21].map(ID);
const FOLDER_B = [22].map(ID);
const FOLDER_ID_A = '7100000000000000001';
const FOLDER_ID_B = '7100000000000000002';

let site;
let daemon;
let home;
let owner = '';
let deliverToken = '';

async function api(method, path, body, token = owner) {
  const res = await fetch('http://127.0.0.1:' + PORT + path, {
    method,
    headers: { authorization: 'Bearer ' + token, 'content-type': 'application/json' },
    body: body && JSON.stringify(body),
    signal: AbortSignal.timeout(5000),
  });
  return { status: res.status, json: res.status === 204 ? null : await res.json() };
}

before(async () => {
  site = await startSite();
  if (!BIN) return;
  home = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'usuyo-userscript-'));
  daemon = spawn(BIN, ['daemon', '--port', String(PORT), '--first-library', join(home, 'lib')], {
    env: { ...process.env, USUYO_HOME: home },
    stdio: 'ignore',
  });
  await until(() => existsSync(join(home, 'api-token')), 15000);
  owner = readFileSync(join(home, 'api-token'), 'utf8').trim();
  await until(() => owner.length > 0);
  let created;
  for (let i = 0; i < 50 && !created; i++) {
    try {
      created = await api('POST', '/deliver-tokens', { name: '抖音油猴脚本（测试）', kind: 'extension' });
    } catch {
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  assert.equal(created.status, 200);
  deliverToken = created.json.token;
});

after(async () => {
  await site.close();
  if (daemon) {
    daemon.kill('SIGTERM');
    await new Promise((r) => daemon.once('exit', r));
  }
  if (home) spawnSync('trash', [home], { timeout: 10000 });
});

const mount = (opts = {}) => load({ site: site.origin, ...opts });
const API = 'https://www.douyin.com/aweme/v1/web/';
const settle = () => new Promise((r) => setTimeout(r, 60));

/** 让「页面」自己请求喜欢的两页，跟真页面往下翻一样。 */
async function browseLikes(s) {
  await s.page.fetch(API + 'aweme/favorite/?device_platform=webapp&count=18&max_cursor=0');
  await s.page.fetch(API + 'aweme/favorite/?device_platform=webapp&count=18&max_cursor=1730000100000');
  await settle();
}

/** 收藏页：整个收藏、收藏夹的名字、两个收藏夹里的作品。 */
async function browseCollections(s) {
  await s.page.fetch(API + 'aweme/listcollection/?device_platform=webapp', { method: 'POST', body: 'cursor=0&count=20' });
  await s.page.fetch(API + 'collects/list/?cursor=0&count=20');
  await s.page.fetch(API + 'collects/video/list/?collects_id=' + FOLDER_ID_A + '&cursor=0&count=20');
  await s.page.fetch(API + 'collects/video/list/?collects_id=' + FOLDER_ID_B + '&cursor=0&count=20');
  await settle();
}

const keys = (o) => Object.keys(o).sort();

/** 点交给，在浏览器的对话框里贴令牌，等到不再是「先确认…」。 */
async function handOver(s, token) {
  if (token) s.answerPrompt(token);
  s.click('交给 usuyo');
  await until(() => !/先确认/.test(s.note()));
}

/** 每个测试最后都查：令牌没进页面的存储、没出现在面板上，脚本只连了 127.0.0.1。 */
function noLeak(s, token, port) {
  const secret = token.split('.')[1];
  assert.ok(!s.panelText().includes(secret), 'secret shown in the panel');
  assert.ok(!s.hasInput(), 'an input element in the page');
  for (const v of s.pageStorage.values()) assert.ok(!v.includes(secret), 'token in page storage');
  assert.deepEqual(s.violations, []);
  for (const r of s.gmRequests) assert.equal(new URL(r.url).host, '127.0.0.1:' + port);
}

// ---------- 解析 ----------

test('parses the likes answer: both shapes, fields, de-duplication, skipped kinds', async () => {
  const s = mount({ port: 1 });
  await browseLikes(s);
  const { like, collect } = s.gm.get('items');
  assert.deepEqual(keys(like), LIKES);
  assert.deepEqual(collect, {});
  // 第二页（包在 data 里）重复了图文那一条，只记一条
  assert.deepEqual(like[ID(1)], { title: '示例喜欢一号', up: '示例作者甲', folder: '', at: 1730000100 * 1000, note: false });
  // 有 images 的是图文；没有 desc 的标题是空的；毫秒时间照用
  assert.equal(like[ID(2)].note, true);
  assert.equal(like[ID(4)].title, '');
  assert.equal(like[ID(5)].at, 1730000500000);
  assert.match(s.panelText(), /喜欢 4（已到底） · 收藏 0/);
  // 已删除的 1 条、没有 aweme_id 的 1 条，各自数出来
  assert.match(s.panelText(), /跳过：缺 id 的 1 条、已删除或不公开的 1 条/);
  assert.equal(s.gmRequests.length, 0, 'recording sends nothing');
  assert.equal(site.seen.filter((u) => u.includes('favorite')).length >= 2, true);
  assert.equal(s.pageStorage.size, 0, 'nothing written to page storage');
});

test('parses 收藏, folder names and the works in each folder', async () => {
  const s = mount({ port: 1 });
  await browseCollections(s);
  const { like, collect, names } = s.gm.get('items');
  assert.deepEqual(like, {});
  assert.deepEqual(keys(collect), [...PLAIN_COLLECT, ...FOLDER_A, ...FOLDER_B].sort());
  // 没有标识的收藏夹不记名字
  assert.deepEqual(names, { [FOLDER_ID_A]: '示例收藏夹甲', [FOLDER_ID_B]: '示例收藏夹乙' });
  assert.equal(collect[ID(11)].at, 1731000100 * 1000, 'collect_time');
  // 第 12 条先在整个收藏里出现，后来在收藏夹里出现：记下它在哪个夹
  assert.equal(collect[ID(12)].folder, FOLDER_ID_A);
  assert.equal(collect[ID(21)].folder, FOLDER_ID_A);
  assert.equal(collect[ID(22)].folder, FOLDER_ID_B);
  assert.equal(collect[ID(11)].folder, '');
  // 不公开（private_status 不为 0）、已删除的不记
  assert.ok(!(ID(13) in collect) && !(ID(14) in collect));
  assert.match(s.panelText(), /收藏 5（已到底）/);
  assert.match(s.panelText(), /跳过：缺 id 的 0 条、已删除或不公开的 2 条/);
});

test('skipped entries are counted once however often the page asks again', async () => {
  const s = mount({ port: 1 });
  await browseLikes(s);
  await browseLikes(s);
  assert.match(s.panelText(), /跳过：缺 id 的 1 条、已删除或不公开的 1 条/);
  assert.equal(Object.keys(s.gm.get('items').like).length, 4);
});

test('the page still gets the untouched answer', async () => {
  const s = mount({ port: 1 });
  const res = await s.page.fetch(API + 'aweme/listcollection/?x=1');
  const body = await res.json();
  assert.equal(body.aweme_list.length, 5);
  assert.equal(res.status, 200);
});

test('reads answers the page gets through XMLHttpRequest too (text and json response types)', async () => {
  const s = mount({ port: 1 });
  for (const responseType of ['', 'json']) {
    const x = new s.page.XMLHttpRequest();
    x.responseType = responseType;
    x.open('GET', API + 'aweme/favorite/?max_cursor=0');
    x.send();
  }
  await until(() => s.gm.get('items'));
  await settle();
  assert.deepEqual(keys(s.gm.get('items').like), [1, 2, 4].map(ID));
});

test('only the site\'s own answers count: other hosts and other interfaces are ignored', async () => {
  const s = mount({ port: 1 });
  await s.page.fetch('https://tracker.example.invalid/aweme/v1/web/aweme/favorite/');
  await s.page.fetch(API + 'aweme/post/?sec_user_id=x');
  await s.page.fetch(API + 'tab/feed/');
  await settle();
  assert.equal(s.gm.get('items'), undefined);
});

test('an error status_code from a list says so and records nothing', async () => {
  const s = mount({ port: 1 });
  await s.page.fetch(API + 'collects/video/list/?collects_id=666&cursor=0');
  await settle();
  assert.equal(s.gm.get('items'), undefined);
  assert.match(s.panelText(), /抖音回了 status_code 8，先别往下翻了/);
});

test('a likes list that cannot be seen (aweme_list null, no has_more) is a recognised empty list', async () => {
  const s = mount({ port: 1 });
  await s.page.fetch(API + 'aweme/favorite/?private=1');
  await settle();
  assert.match(s.panelText(), /喜欢 0 · 收藏 0/);
  const d = JSON.parse(await copyOut(s));
  assert.equal(d.recognised, 1);
  assert.deepEqual(d.answers.map((a) => [a.path, a.recognised, a.took]), [['/aweme/v1/web/aweme/favorite/', true, 0]]);
});

test('an entry without an id is skipped, fields that are absent are tolerated', async () => {
  const s = mount({ port: 1 });
  await s.page.fetch(API + 'aweme/listcollection/?odd=1');
  await settle();
  assert.deepEqual(s.gm.get('items').collect, { [ID(90)]: { title: '', up: '', folder: '', at: 0, note: false } });
  assert.match(s.panelText(), /缺 id 的 4 条/);
});

// ---------- 诊断（复制诊断） ----------

const text = readFileSync(SCRIPT_PATH, 'utf8');
const DIAG_START = '  // ---------- 诊断：只记回答的形状，不记任何值 ----------';
const DIAG_END = '  // ---------- 诊断结束 ----------';
const wait = (ms = 30) => new Promise((r) => setTimeout(r, ms));
const fixtureText = (name) => readFileSync(new URL('./fixtures/' + name, import.meta.url), 'utf8');

function diagBlock(source) {
  const start = source.indexOf(DIAG_START);
  const end = source.indexOf(DIAG_END);
  assert.ok(start >= 0 && end > start, 'diagnosis block markers');
  return source.slice(start, end + DIAG_END.length);
}

/** 页面的 fetch 换成固定回答，并记下被调的地址。 */
function answering(body, status = 200) {
  const calls = [];
  const fetchImpl = async (input) => {
    calls.push(String(input));
    return new Response(typeof body === 'string' ? body : JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
  };
  return { fetchImpl, calls };
}

async function copyOut(s) {
  s.click('复制诊断');
  await wait(5);
  return s.copied[s.copied.length - 1];
}

function everyString(value, out = [], withKeys = false) {
  if (typeof value === 'string') out.push(value);
  else if (value && typeof value === 'object') {
    for (const [k, v] of Object.entries(value)) {
      if (withKeys) out.push(k);
      everyString(v, out, withKeys);
    }
  }
  return out;
}

test("the diagnosis block is the one in the Bilibili family, and the shapes reducer is the probe's", () => {
  const here = diagBlock(text);
  let compared = 0;
  for (const sibling of ['../../../bilibili/userscript/usuyo-bilibili-saved.user.js', '../../../xiaohongshu/userscript/usuyo-xhs-saved.user.js', '../../../x/userscript/usuyo-x-saved.user.js']) {
    const url = new URL(sibling, import.meta.url);
    if (!existsSync(url)) continue;
    assert.equal(here, diagBlock(readFileSync(url, 'utf8')), sibling);
    compared++;
  }
  const probe = new URL('../../../wechat-channels/userscript/usuyo-sph-probe.user.js', import.meta.url);
  if (existsSync(probe)) {
    // 探针用双引号，其余逐字相同：归约函数的正文去掉引号差别后一样
    const norm = (s) => s.replace(/"/g, "'").replace(/\s+/g, ' ');
    const probeText = readFileSync(probe, 'utf8');
    const part = (s, a, b) => s.slice(s.indexOf(a), s.indexOf(b, s.indexOf(a)));
    // 家族里的这一份没有探针里 worker 专用的 safeName，其余的函数正文一样
    for (const [from, to] of [['function safeKey', 'function safeName'], ['function tryJson', '// ---------- paths ----------']]) {
      assert.ok(norm(part(here, from, to === 'function safeName' ? 'function tryJson' : to)) === norm(part(probeText, from, to)), 'reducer differs from the probe at ' + from);
    }
    compared++;
  }
  assert.ok(compared >= 1, 'no sibling script found to compare with');
});

// mulberry32：小的种子也给出分散的头几个值
function rng(seed) {
  let t0 = seed >>> 0;
  return () => {
    t0 = (t0 + 0x6d2b79f5) >>> 0;
    let t = t0;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 2 ** 32;
  };
}

// 代表内容的字符串：id、url、名字、令牌、正文、里面带 JSON 的字符串
function markerString(r, i) {
  const kinds = [
    () => 'MARK' + i + 'x' + Math.floor(r() * 1e9),
    () => 'https://mark' + i + '.example.invalid/v?token=MARK' + i,
    () => String(7312345678901234567n + BigInt(i)),
    () => 'v2_0600002310' + i + 'MARK@finder',
    () => '标记内容' + i + ' MARK' + i,
    () => '{"inner":"MARKJSON' + i + '","n":' + i + '}',
  ];
  return kinds[Math.floor(r() * kinds.length)]();
}

function randomValue(r, depth, markers) {
  const pick = r();
  if (depth > 4 || pick < 0.35) {
    const m = markerString(r, markers.length);
    markers.push(m);
    return m;
  }
  if (pick < 0.45) {
    const n = 900000000 + Math.floor(r() * 99999999);
    markers.push(String(n));
    return n;
  }
  if (pick < 0.7) return Array.from({ length: Math.floor(r() * 4) }, () => randomValue(r, depth + 1, markers));
  const obj = {};
  const n = 1 + Math.floor(r() * 4);
  for (let i = 0; i < n; i++) {
    // 字段名一样的键，和长得像内容的键（id、中文、url），后一种不能出现在诊断里
    if (r() < 0.5) {
      obj['field' + String.fromCharCode(97 + i)] = randomValue(r, depth + 1, markers);
    } else {
      const k = markerString(r, markers.length);
      markers.push(k);
      obj[k] = randomValue(r, depth + 1, markers);
    }
  }
  return obj;
}

// 已知的限度（探针和其它几个脚本一样）：归约函数只丢「长得像内容」的键，不丢长得像字段名的键。
// 一个至多 40 个字符、至多 3 个数字、不是长十六进制的标识符式的键会原样留在诊断里；
// 所以这里种的键都是长得像内容的。路径里的段另外由 diagPath 把所有带数字的段打成 :x。
test('the diagnosis never holds a planted value, content-like key or path id', async () => {
  let nested = 0;
  for (let seed = 1; seed <= 300; seed++) {
    const r = rng(seed);
    const markers = [];
    const body = { root: randomValue(r, 1, markers), [markerString(r, -1)]: randomValue(r, 1, markers) };
    markers.push(Object.keys(body)[1]);
    if (JSON.stringify(body).includes('field')) nested++;
    const { fetchImpl } = answering(body);
    const s = mount({ port: 1, fetchImpl });
    await s.page.fetch('https://www.douyin.com/aweme/v1/web/aweme/MARKID9abc/favorite/?cursor=MARKCURSOR&token=MARKQ');
    await s.page.fetch('https://www.douyin.com/aweme/v1/web/collect/MARKword' + 'x'.repeat(30));
    await wait(5);
    const out = await copyOut(s);
    for (const m of [...markers, 'MARKID9abc', 'MARKQ', 'MARKCURSOR']) {
      assert.ok(!out.includes(m), `seed ${seed}: ${JSON.stringify(m)} leaked into ${out}`);
    }
    assert.ok(!/MARK|标记|\bmark\d/.test(out), `seed ${seed}: marker fragment leaked into ${out}`);
  }
  assert.ok(nested > 100, 'generator produced nested objects');
});

test('the diagnosis keeps key paths, types and lengths of an answer', async () => {
  const { fetchImpl } = answering({ status_code: 0, other: { items: [{ name: 'hello' }, { name: 'hi', url: 'x' }], count: 4 } });
  const s = mount({ port: 1, fetchImpl });
  await s.page.fetch(API + 'aweme/favorite/');
  await wait();
  const d = JSON.parse(await copyOut(s));
  assert.deepEqual(d.answers[0].shape, { status_code: 'number', other: { items: { $array: 2, $items: { name: 'string:2..5', url: 'string:1' } }, count: 'number' } });
});

test('repeats merge and the number of kept answers is bounded', async () => {
  const { fetchImpl } = answering({ status_code: 0 });
  const s = mount({ port: 1, fetchImpl });
  const name = (i) => 'aweme/v1/web/collect/' + String.fromCharCode(97 + (i % 26)) + String.fromCharCode(97 + Math.floor(i / 26));
  for (let i = 0; i < 50; i++) await s.page.fetch('https://www.douyin.com/' + name(i));
  await wait(60);
  let d = JSON.parse(await copyOut(s));
  assert.equal(d.answers.length, 20);
  assert.equal(d.looked, 50);
  assert.equal(d.dropped, 30);
  for (let i = 0; i < 5; i++) await s.page.fetch('https://www.douyin.com/' + name(49) + '?cursor=' + i);
  await wait(60);
  d = JSON.parse(await copyOut(s));
  assert.equal(d.answers.length, 20);
  const last = d.answers[d.answers.length - 1];
  assert.equal(last.path, '/' + name(49));
  assert.equal(last.answers, 6);
});

test('a recognised list answer records its counts and the kind of page', async () => {
  const s = mount({ port: 1 });
  await browseLikes(s);
  await browseCollections(s);
  const d = JSON.parse(await copyOut(s));
  assert.equal(d.tool, 'usuyo-douyin-saved');
  assert.equal(d.version, '0.1.0');
  assert.equal(d.page, 'self');
  assert.equal(d.startedAs, 'loading');
  assert.equal(d.recognised, 6);
  const like = d.answers.find((a) => a.path === '/aweme/v1/web/aweme/favorite/');
  assert.deepEqual({ status: like.status, json: like.json, recognised: like.recognised, took: like.took, answers: like.answers }, { status: 200, json: true, recognised: true, took: 5, answers: 2 });
  assert.deepEqual(like.skipped, { badId: 1, removed: 1 });
  assert.deepEqual(like.stopped, []);
  assert.ok(like.shape.aweme_list.$items.author.nickname !== undefined);
  const collect = d.answers.find((a) => a.path === '/aweme/v1/web/aweme/listcollection/');
  assert.deepEqual([collect.took, collect.skipped], [3, { badId: 0, removed: 2 }]);
  assert.equal(d.answers.find((a) => a.path === '/aweme/v1/web/collects/list/').took, 2);
  assert.equal(d.answers.find((a) => a.path === '/aweme/v1/web/collects/video/list/').answers, 2);
});

test('an answer on a path the script does not know, but carrying a list, is kept and flagged not recognised', async () => {
  const s = mount({ port: 1 });
  // 一个忙的页面：别的接口的回答很多，不能把它挤掉；不带列表、名字也不像的回答根本不留
  for (let i = 0; i < 60; i++) await s.page.fetch(API + 'user/profile/other' + (i % 7) + '/');
  await s.page.fetch(API + 'mystery/saved/');
  for (let i = 0; i < 60; i++) await s.page.fetch(API + 'user/profile/other' + (i % 7) + '/');
  await wait(60);
  const d = JSON.parse(await copyOut(s));
  const row = d.answers.find((a) => a.path === '/aweme/v1/web/mystery/saved/');
  assert.ok(row, 'the list-bearing answer is in the diagnosis');
  assert.equal(row.recognised, false);
  assert.equal(row.took, 0);
  assert.equal(d.recognised, 0);
  assert.equal(d.answers.length, 1, 'answers with no list and no list-like name take no slot');
  assert.equal(s.gm.get('items'), undefined, 'nothing is recorded from a path the script does not know');
});

test('the right path with the list under another name is not recognised, and the quiet sentence still comes', async (t) => {
  const realNow = Date.now;
  t.after(() => (Date.now = realNow));
  const s = mount({ port: 1 });
  await s.page.fetch(API + 'aweme/favorite/?renamed=1');
  await wait();
  const base = realNow();
  Date.now = () => base + 31000;
  s.tick();
  assert.match(s.note(), /还没认出喜欢或收藏的回答.*复制诊断/);
  const d = JSON.parse(await copyOut(s));
  assert.equal(d.recognised, 0);
  assert.deepEqual(d.answers.map((a) => [a.path, a.recognised, a.took]), [['/aweme/v1/web/aweme/favorite/', false, 0]]);
  assert.equal(s.gm.get('items'), undefined);
});

test('an error code or a verification page on a list path is recorded as not recognised', async () => {
  for (const [body, status, stopped] of [
    [{ status_code: 8, status_msg: 'x' }, 200, ['errorCode']],
    ['<html>verify</html>', 403, []],
  ]) {
    const { fetchImpl } = answering(body, status);
    const s = mount({ port: 1, fetchImpl });
    await s.page.fetch(API + 'aweme/listcollection/');
    await wait();
    const d = JSON.parse(await copyOut(s));
    assert.equal(d.recognised, 0);
    const a = d.answers[0];
    assert.deepEqual({ status: a.status, json: a.json, recognised: a.recognised, took: a.took, stopped: a.stopped }, { status, json: typeof body !== 'string', recognised: false, took: 0, stopped });
  }
});

test('answers from other hosts and outside the interface path are never read', async () => {
  let read = 0;
  const fetchImpl = async () => {
    const res = new Response('{"aweme_list":[]}', { headers: { 'content-type': 'application/json' } });
    const clone = res.clone.bind(res);
    res.clone = () => (read++, clone());
    return res;
  };
  const s = mount({ port: 1, fetchImpl });
  await s.page.fetch('https://tracker.example.invalid/aweme/v1/web/aweme/favorite/');
  await s.page.fetch('https://www.douyin.com/some/page.json');
  await s.page.fetch('https://www.douyin.com/aweme/v1/web/aweme/favorite/');
  await wait();
  assert.equal(read, 1, "only the site's own interface answer is read");
});

test('an xhr answer is recorded the same way', async () => {
  const s = mount({ port: 1 });
  const x = new s.page.XMLHttpRequest();
  x.open('GET', API + 'aweme/listcollection/');
  x.send();
  await until(() => s.gm.get('items'));
  const d = JSON.parse(await copyOut(s));
  assert.deepEqual(d.answers.map((a) => [a.path, a.status, a.recognised, a.took]), [['/aweme/v1/web/aweme/listcollection/', 200, true, 3]]);
});

test('copying puts the text on the clipboard and sends it nowhere else', async () => {
  const { fetchImpl, calls } = answering({ status_code: 0, aweme_list: [] });
  const s = mount({ port: 1, fetchImpl });
  await s.page.fetch(API + 'aweme/favorite/');
  await wait();
  const items = structuredClone(s.gm.get('items'));
  const out = await copyOut(s);
  assert.equal(s.copied.length, 1);
  assert.equal(typeof JSON.parse(out).version, 'string');
  assert.match(s.note(), /诊断已复制，只含结构/);
  assert.match(s.panelText(), /诊断里只有回答的结构，没有任何内容/);
  assert.equal(s.paste().style.display, 'none');
  assert.deepEqual(s.gmRequests, []);
  assert.deepEqual(s.violations, []);
  assert.equal(s.pageStorage.size, 0);
  assert.deepEqual(s.gm.get('items'), items);
  assert.equal(calls.length, 1, 'no request beyond the page own');
  assert.ok(!s.hasInput());
});

test('a refused or missing clipboard falls back to a selected text box', async () => {
  for (const clipboard of ['refuse', 'none']) {
    const s = mount({ port: 1, clipboard });
    await browseLikes(s);
    s.click('复制诊断');
    await wait(5);
    const box = s.paste();
    assert.equal(box.style.display, 'block');
    assert.equal(box.selected, true);
    assert.equal(box.readOnly, true);
    assert.equal(JSON.parse(box.value).tool, 'usuyo-douyin-saved');
    assert.match(s.note(), /没能写进剪贴板/);
    assert.deepEqual(s.copied, []);
    assert.deepEqual(s.gmRequests, []);
  }
});

test('the diagnosis holds none of the fixture texts, names or ids, nor the planted markers', async () => {
  const s = mount({ port: 1 });
  await browseLikes(s);
  await browseCollections(s);
  await s.page.fetch(API + 'tab/feed/?token=MARKQUERY_1');
  await s.page.fetch(API + 'mystery/saved/');
  await s.page.fetch(API + 'user/MARK1234567890x/profile/');
  await s.page.fetch(API + 'aweme/favorite/?renamed=1');
  await settle();
  const out = await copyOut(s);
  assert.deepEqual(s.gmRequests, [], 'copying the diagnosis sends nothing');
  for (const marker of ['MARK48210937x1', 'MARKVALUE_77aa', 'MARKTOKEN_5d20', 'mark.example.invalid', 'MARKQUERY_1', 'MARK1234567890x']) {
    assert.ok(!out.includes(marker), 'leaked ' + marker);
  }
  // 所有 fixtures 里的字符串值（_note 除外的每一个）一个都不能出现；键只查长得像内容的
  const values = new Set();
  const contentKeys = new Set();
  for (const f of readdirSync(new URL('./fixtures/', import.meta.url))) {
    const json = JSON.parse(fixtureText(f));
    delete json._note;
    for (const v of everyString(json)) values.add(v);
    for (const k of everyString(json, [], true)) if (!/^[A-Za-z_]{1,30}$/.test(k)) contentKeys.add(k);
  }
  values.delete('0');
  // 太短、和字段名一样的词不查
  const SHORT_WORD = /^(?:[A-Za-z_]{1,6}|[0-9]{1,4})$/;
  assert.ok(values.size > 40);
  for (const v of [...values, ...contentKeys]) {
    if (!v || SHORT_WORD.test(v)) continue;
    assert.ok(!out.includes(v), 'value ' + v + ' in the diagnosis');
  }
  assert.ok(!/示例/.test(out) && !out.includes('73000000000'), 'fixture text or id');
  assert.ok(!/https?:/.test(out), 'no url');
  assert.ok(!out.includes('127.0.0.1'));
});

test('a quiet page says so once, calmly, in the note line, and only until a list answer is recognised', async (t) => {
  const realNow = Date.now;
  t.after(() => (Date.now = realNow));
  const s = mount({ port: 1 });
  assert.match(s.note(), /自己往下翻/);
  const base = realNow();
  Date.now = () => base + 31000;
  s.tick();
  assert.match(s.note(), /还没认出喜欢或收藏的回答.*复制诊断/);
  assert.ok(!/[0-9]/.test(s.note()), 'no counts in the sentence');
  assert.equal(s.warnText(), '', 'nothing in the warning line');
  await browseLikes(s);
  s.tick();
  assert.doesNotMatch(s.note(), /还没认出/);
  // 不在自己的页上不提
  const other = mount({ port: 1, path: '/video/7300000000000000001' });
  Date.now = () => base + 90000;
  other.tick();
  assert.doesNotMatch(other.note(), /还没认出/);
});

// ---------- 交给 usuyo ----------

test('delivers likes, 收藏 and folders to a throwaway usuyo daemon', { skip: !BIN && 'USUYO_BIN not set' }, async () => {
  const s = mount({ port: PORT });
  await browseLikes(s);
  await browseCollections(s);
  const before = (await api('GET', '/imports')).json.length;
  const pageRequests = site.seen.length;
  await handOver(s, deliverToken);
  assert.equal(site.seen.length, pageRequests, 'delivery sends nothing to Douyin');
  assert.match(s.note(), /交给 usuyo 了：喜欢 4 条、收藏 5 条，到 usuyo 的导入页确认了才会去取/);

  // 握手只发随机数和令牌的 id，令牌只在核对过以后进 /v1/batches 的请求头
  const [hs, ...rest] = s.gmRequests;
  assert.equal(new URL(hs.url).pathname, '/handshake');
  assert.deepEqual(Object.keys(JSON.parse(hs.data)).sort(), ['client', 'nonce']);
  assert.equal(JSON.parse(hs.data).client, deliverToken.split('.')[0]);
  assert.ok(!JSON.stringify(hs).includes(deliverToken.split('.')[1]));
  assert.deepEqual(rest.map((r) => new URL(r.url).pathname), ['/v1/batches', '/v1/batches', '/v1/batches', '/v1/batches']);
  assert.deepEqual(rest.map((r) => JSON.parse(r.data).action), ['like', 'favorite', 'favorite', 'favorite']);

  const imports = (await api('GET', '/imports')).json;
  assert.equal(imports.length, before + 4);
  const byTitle = Object.fromEntries(imports.map((i) => [i.title, i]));
  const day = new Date().toLocaleDateString('sv');
  const expect = [
    ['抖音喜欢 ' + day, LIKES],
    ['抖音收藏 ' + day, PLAIN_COLLECT],
    ['抖音收藏「示例收藏夹甲」 ' + day, FOLDER_A],
    ['抖音收藏「示例收藏夹乙」 ' + day, FOLDER_B],
  ];
  for (const [name, ids] of expect) {
    const imp = byTitle[name];
    assert.ok(imp, 'batch ' + name);
    assert.equal(imp.kind, 'delivered');
    const entries = (await api('GET', '/imports/' + imp.id + '/entries')).json;
    const noteIds = [ID(2)];
    assert.deepEqual(
      entries.map((e) => e.url).sort(),
      ids.map((id) => 'https://www.douyin.com/' + (noteIds.includes(id) ? 'note/' : 'video/') + id).sort(),
    );
    for (const e of entries) assert.equal(e.action, name.includes('喜欢') ? 'like' : 'favorite');
  }
  // 交出去的从脚本里去掉
  assert.deepEqual(s.gm.get('items').like, {});
  assert.deepEqual(s.gm.get('items').collect, {});
  assert.equal(s.gm.get('deliverToken'), deliverToken);
  assert.match(s.panelText(), /令牌 [0-9a-f]{6}… 存在脚本管理器里/);
  noLeak(s, deliverToken, PORT);
});

test('a listener that cannot prove itself gets nothing', async (t) => {
  const fake = await startListener(() => [200, { proof: randomBytes(32).toString('hex') }]);
  t.after(() => fake.close());
  const token = randomBytes(8).toString('hex') + '.' + randomBytes(32).toString('hex');
  const s = mount({ port: fake.port });
  await browseLikes(s);
  await handOver(s, token);
  assert.match(s.note(), /没能证明自己是 usuyo.*令牌没有发出去，什么都没交/);
  assert.deepEqual(fake.seen.map((r) => r.url), ['/handshake']);
  const secret = token.split('.')[1];
  for (const r of fake.seen) {
    assert.ok(!JSON.stringify(r.headers).includes(secret) && !r.body.includes(secret));
    assert.equal(r.headers.authorization, undefined);
  }
  assert.equal(Object.keys(s.gm.get('items').like).length, 4, 'entries stay');
  noLeak(s, token, fake.port);
});

test('a peer that answers the handshake with an error status gets nothing', async (t) => {
  const fake = await startListener(() => [500, {}]);
  t.after(() => fake.close());
  const token = randomBytes(8).toString('hex') + '.' + randomBytes(32).toString('hex');
  const s = mount({ port: fake.port });
  await browseLikes(s);
  await handOver(s, token);
  assert.match(s.note(), /没能证明自己是 usuyo/);
  assert.deepEqual(fake.seen.map((r) => r.url), ['/handshake']);
  noLeak(s, token, fake.port);
});

test('closed port says usuyo is not running', async () => {
  const gone = await startListener(() => [500, {}]);
  const port = gone.port;
  await gone.close();
  const token = randomBytes(8).toString('hex') + '.' + randomBytes(32).toString('hex');
  const s = mount({ port });
  await browseLikes(s);
  await handOver(s, token);
  assert.match(s.note(), /usuyo 没有开着（本机 \d+ 端口没有回应），什么都没交/);
  assert.equal(s.gmRequests.length, 1);
  noLeak(s, token, port);
});

test('a refused token stops the delivery', async (t) => {
  const token = randomBytes(8).toString('hex') + '.' + randomBytes(32).toString('hex');
  // 能答对握手、却不认这个令牌的对面（例如令牌在握手之后被撤掉）
  const fake = await startListener((req, body) => {
    if (req.url === '/handshake') {
      const { nonce } = JSON.parse(body);
      const proof = createHmac('sha256', token).update(`usuyo-daemon-proof/v1\n127.0.0.1:${fake.port}\n${nonce}`).digest('hex');
      return [200, { proof }];
    }
    return [401, { code: 'unauthorized' }];
  });
  t.after(() => fake.close());
  const s = mount({ port: fake.port });
  await browseLikes(s);
  await browseCollections(s);
  await handOver(s, token);
  assert.match(s.note(), /usuyo 不认这个令牌（可能撤掉了），剩下的没有收下/);
  assert.deepEqual(fake.seen.map((r) => r.url), ['/handshake', '/v1/batches'], 'stops after the first refusal');
  assert.equal(Object.keys(s.gm.get('items').like).length, 4, 'entries stay');
  noLeak(s, token, fake.port);
  s.click('忘掉令牌');
  assert.equal(s.gm.get('deliverToken'), '');
});

test('a token revoked in usuyo is never sent to it', { skip: !BIN && 'USUYO_BIN not set' }, async () => {
  const created = await api('POST', '/deliver-tokens', { name: '撤掉的', kind: 'extension' });
  const token = created.json.token;
  assert.equal((await api('DELETE', '/deliver-tokens/' + created.json.id)).status, 204);
  const s = mount({ port: PORT });
  await browseLikes(s);
  await handOver(s, token);
  assert.match(s.note(), /也可能这个令牌在 usuyo 里撤掉了/);
  assert.deepEqual(s.gmRequests.map((r) => new URL(r.url).pathname), ['/handshake']);
  noLeak(s, token, PORT);
});

test('a malformed token is not kept', async () => {
  const s = mount({ port: 1 });
  await browseLikes(s);
  s.answerPrompt('not-a-token');
  s.click('交给 usuyo');
  assert.match(s.note(), /这不像 usuyo 的投递令牌/);
  assert.equal(s.gm.get('deliverToken'), undefined);
  assert.equal(s.gmRequests.length, 0);
});

test('nothing noted yet: no prompt, no request', async () => {
  const s = mount({ port: 1 });
  s.click('交给 usuyo');
  assert.match(s.note(), /还没有记下的条目/);
  assert.equal(s.prompted.length, 0);
  assert.equal(s.gmRequests.length, 0);
});

// 脚本管理器替脚本带上的 Origin：扩展的来源能进，页面的来源不能
for (const origin of ['chrome-extension://abcdefghijklmnop', 'moz-extension://0b1e5a8c-1111-4222-8333-944445555666', 'safari-web-extension://0B1E5A8C-1111-4222-8333-944445555666']) {
  test('delivers with Origin ' + origin.split(':')[0], { skip: !BIN && 'USUYO_BIN not set' }, async () => {
    const s = mount({ port: PORT, origin });
    await browseLikes(s);
    await handOver(s, deliverToken);
    assert.match(s.note(), /交给 usuyo 了：喜欢 4 条、收藏 0 条/);
    noLeak(s, deliverToken, PORT);
  });
}

test('a page Origin is turned away before any token is sent', { skip: !BIN && 'USUYO_BIN not set' }, async () => {
  const s = mount({ port: PORT, origin: 'https://www.douyin.com' });
  await browseLikes(s);
  await handOver(s, deliverToken);
  assert.match(s.note(), /usuyo 没让这个脚本管理器的请求进来，什么都没交/);
  assert.deepEqual(s.gmRequests.map((r) => new URL(r.url).pathname), ['/handshake']);
  noLeak(s, deliverToken, PORT);
});

test('the token can be entered and forgotten from the script manager menu', async () => {
  const token = randomBytes(8).toString('hex') + '.' + randomBytes(32).toString('hex');
  const s = mount({ port: 1 });
  assert.deepEqual(s.menuLabels(), ['填 usuyo 令牌', '忘掉 usuyo 令牌']);
  await browseLikes(s);
  // 对话框没弹出来或被关掉（prompt 回 null）
  s.click('交给 usuyo');
  assert.match(s.note(), /没拿到令牌.*脚本管理器的菜单里点「填 usuyo 令牌」/);
  assert.equal(s.gmRequests.length, 0);
  s.answerPrompt('  ' + token + '\n');
  s.menu('填 usuyo 令牌');
  assert.equal(s.gm.get('deliverToken'), token, 'kept, trimmed');
  // 面板上只有 id 的前 6 位
  assert.ok(s.panelText().includes('令牌 ' + token.slice(0, 6) + '…'));
  assert.ok(!s.panelText().includes(token.slice(6, 16)) && !s.panelText().includes(token.slice(17, 25)));
  s.menu('忘掉 usuyo 令牌');
  assert.equal(s.gm.get('deliverToken'), '');
  assert.ok(!s.hasInput());
});

// 页面和脚本在同一个环境里时，页面能改掉的内置方法：换成会记账的间谍
function plant(page, secret) {
  const hits = [];
  const has = String.prototype.indexOf;
  const values = Object.values;
  let inside = false;
  // 只算脚本自己直接调的：脚本管理器那一侧（测试里是 harness 的 GM_xmlhttpRequest 和 node:http）不算
  const fromScript = () => {
    const st = new Error().stack;
    // 栈：Error、fromScript、间谍自己，再下一帧才是调它的
    const own = Reflect.apply(has, st, ['\n    at ']);
    const spyFrame = Reflect.apply(has, st, ['\n    at ', own + 1]);
    const caller = Reflect.apply(has, st, ['\n    at ', spyFrame + 1]);
    const next = Reflect.apply(has, st, ['\n', caller + 1]);
    const at = Reflect.apply(has, st, [SCRIPT_FRAME, caller]);
    return caller >= 0 && at >= 0 && (next < 0 || at < next);
  };
  const carries = (v, depth = 0) => {
    if (typeof v === 'string') return Reflect.apply(has, v, [secret]) >= 0;
    if (v && typeof v === 'object' && depth < 2) {
      try {
        return values(v).some((x) => carries(x, depth + 1));
      } catch {
        return false;
      }
    }
    return false;
  };
  const restores = [];
  const spy = (obj, name, label) => {
    const orig = obj[name];
    obj[name] = function (...args) {
      if (!inside) {
        inside = true;
        try {
          if (fromScript() && (carries(this) || args.some((a) => carries(a)))) hits.push(label);
        } finally {
          inside = false;
        }
      }
      return new.target ? Reflect.construct(orig, args, new.target) : Reflect.apply(orig, this, args);
    };
    restores.push(() => (obj[name] = orig));
  };
  for (const n of ['exec', 'test']) spy(RegExp.prototype, n, 'RegExp.' + n);
  for (const n of ['trim', 'indexOf', 'slice', 'charCodeAt', 'split', 'substring']) spy(String.prototype, n, 'String.' + n);
  spy(TextEncoder.prototype, 'encode', 'TextEncoder.encode');
  spy(JSON, 'stringify', 'JSON.stringify');
  for (const n of ['call', 'apply', 'bind']) spy(Function.prototype, n, 'Function.' + n);
  const promptCalls = [];
  const pagePrompt = page.prompt;
  page.prompt = (...a) => {
    promptCalls.push(a);
    return null;
  };
  restores.push(() => (page.prompt = pagePrompt));
  return { hits, promptCalls, restore: () => restores.reverse().forEach((r) => r()) };
}

test('page-patched builtins never see the token', { skip: !BIN && 'USUYO_BIN not set' }, async () => {
  const s = mount({ port: PORT });
  await browseLikes(s);
  const secret = deliverToken.split('.')[1];
  const spies = plant(s.page, secret);
  try {
    s.answerPrompt(' ' + deliverToken + ' ');
    s.click('交给 usuyo');
    await until(() => !/先确认|令牌存着了/.test(s.note()));
  } finally {
    spies.restore();
  }
  assert.match(s.note(), /交给 usuyo 了：喜欢 4 条、收藏 0 条/);
  assert.deepEqual(spies.hits, [], 'a patched builtin saw the token');
  assert.deepEqual(spies.promptCalls, [], 'the page prompt was used');
  assert.equal(s.prompted.length, 1, 'the prompt captured at start was used');
  assert.ok(!s.hasInput());
  noLeak(s, deliverToken, PORT);
});

// ---------- 脚本文字 ----------

test('script names no address other than 127.0.0.1, the matched page and the project address', () => {
  const allowed = [
    'https://www.douyin.com/user/self*',
    'https://github.com/usuyo-fetch/douyin',
    // 交出去的链接：视频和图文
    'https://www.douyin.com/video/',
    'https://www.douyin.com/note/',
    'http://127.0.0.1:',
    '127.0.0.1',
  ];
  let rest = text;
  for (const a of allowed) rest = rest.split(a).join('');
  assert.ok(!/[a-z][a-z0-9+.-]*:\/\//i.test(rest), 'no other scheme:// address');
  assert.ok(!/\b[a-z0-9-]+(?:\.[a-z0-9-]+)*\.(?:com|cn|net|org|io|dev|app|invalid)\b/i.test(rest), 'no other host name');
  assert.deepEqual(text.match(/^\/\/ @connect\s+.*$/gm), ['// @connect      127.0.0.1']);
  assert.deepEqual(text.match(/^\/\/ @(?:match|include|exclude)\s+.*$/gm), ['// @match        https://www.douyin.com/user/self*']);
  assert.ok(/^\/\/ @run-at\s+document-start$/m.test(text));
  assert.ok(text.includes('https://github.com/usuyo-fetch/douyin'));
});

test('script reads no cookie or page storage, and never clicks or scrolls the page', () => {
  for (const banned of ['document.cookie', 'localStorage.', 'sessionStorage.', 'indexedDB', 'cookieStore', 'sendBeacon', 'scrollTo', 'scrollBy', 'scrollIntoView', 'dispatchEvent', 'WebSocket']) {
    assert.ok(!text.includes(banned), `script contains ${banned}`);
  }
  // 脚本自己不发页面的请求：fetch 只出现在「包一层页面的 fetch」里
  assert.deepEqual(text.match(/\boriginalFetch\.apply\(/g)?.length, 1);
  assert.ok(!/\bfetch\(/.test(text.replace(/page\.fetch = function/, '')), 'no direct fetch call');
});
