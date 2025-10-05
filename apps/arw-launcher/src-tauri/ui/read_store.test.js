#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const vm = require('vm');
const assert = require('assert');

function makeNode(tag = 'div') {
  const node = {
    tagName: String(tag || 'div').toUpperCase(),
    id: '',
    style: {},
    className: '',
    dataset: {},
    attributes: Object.create(null),
    children: [],
    parentNode: null,
    value: '',
    classList: {
      add(cls) {
        if (!cls) return;
        const parts = new Set(String(node.className || '').split(/\s+/).filter(Boolean));
        parts.add(cls);
        node.className = Array.from(parts).join(' ');
      },
      remove(cls) {
        if (!cls) return;
        const parts = new Set(String(node.className || '').split(/\s+/).filter(Boolean));
        parts.delete(cls);
        node.className = Array.from(parts).join(' ');
      },
      toggle(cls, force) {
        if (force === undefined) {
          this.contains(cls) ? this.remove(cls) : this.add(cls);
        } else if (force) {
          this.add(cls);
        } else {
          this.remove(cls);
        }
      },
      contains(cls) {
        const parts = new Set(String(node.className || '').split(/\s+/).filter(Boolean));
        return parts.has(cls);
      }
    },
    appendChild(child) {
      if (!child) return child;
      child.parentNode = node;
      node.children.push(child);
      return child;
    },
    prepend(child) {
      if (!child) return child;
      child.parentNode = node;
      node.children.unshift(child);
      return child;
    },
    removeChild(child) {
      node.children = node.children.filter((c) => c !== child);
      if (child) child.parentNode = null;
    },
    insertAdjacentHTML() {},
    setAttribute(name, value) {
      const val = String(value ?? '');
      node.attributes[name] = val;
      if (name === 'id') node.id = val;
      if (name.startsWith('data-')) {
        const key = name.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase());
        node.dataset[key] = val;
      }
    },
    getAttribute(name) {
      return Object.prototype.hasOwnProperty.call(node.attributes, name)
        ? node.attributes[name]
        : null;
    },
    addEventListener() {},
    removeEventListener() {},
    querySelector(selector) {
      return node.querySelectorAll(selector)[0] || null;
    },
    querySelectorAll(selector) {
      if (!selector) return [];
      const matches = [];
      const check = (candidate) => {
        if (!candidate || typeof candidate.getAttribute !== 'function') return false;
        if (selector === '[data-bar]') return candidate.getAttribute('data-bar') !== null;
        if (selector === '[data-text]') return candidate.getAttribute('data-text') !== null;
        const rowMatch = selector.match(/^\[data-row="(.+)"\]$/);
        if (rowMatch) return candidate.getAttribute('data-row') === rowMatch[1];
        return false;
      };
      const walk = (current) => {
        if (check(current)) matches.push(current);
        for (const child of current.children || []) walk(child);
      };
      walk(node);
      return matches;
    },
    cloneNode() { return makeNode(tag); },
    focus() {},
    blur() {},
    submit() {},
    click() {},
  };
  Object.defineProperty(node, 'innerHTML', {
    get() { return node._innerHTML || ''; },
    set(value) {
      node._innerHTML = String(value ?? '');
      node.children = [];
      if (node._innerHTML.includes('data-bar')) {
        const label = makeNode('div');
        label.className = 'mono';
        node.appendChild(label);
        const barWrap = makeNode('div');
        barWrap.className = 'bar';
        const fill = makeNode('i');
        fill.setAttribute('data-bar', '');
        barWrap.appendChild(fill);
        node.appendChild(barWrap);
        const meta = makeNode('div');
        meta.className = 'mono dim';
        meta.setAttribute('data-text', '');
        node.appendChild(meta);
      }
    },
  });
  Object.defineProperty(node, 'textContent', {
    get() { return node._textContent || ''; },
    set(value) { node._textContent = String(value ?? ''); },
  });
  return node;
}

const documentNode = makeNode();
documentNode.body = makeNode();
documentNode.documentElement = makeNode();
documentNode.createElement = () => makeNode();
documentNode.getElementById = () => makeNode();
documentNode.querySelector = () => makeNode();
documentNode.querySelectorAll = () => [];
documentNode.addEventListener = () => {};
documentNode.removeEventListener = () => {};

const storage = new Map();
const localStorage = {
  getItem(key) { return storage.has(key) ? storage.get(key) : null; },
  setItem(key, value) { storage.set(key, String(value)); },
  removeItem(key) { storage.delete(key); },
};

const navigatorObj = {
  clipboard: {
    async writeText() { return; },
  },
};

const fetchStub = async () => ({
  ok: true,
  json: async () => ({}),
  text: async () => '',
});

const windowObj = {
  location: { href: 'http://localhost/index.html', pathname: '/index.html' },
  document: documentNode,
  navigator: navigatorObj,
  localStorage,
  console,
  setTimeout,
  clearTimeout,
  setInterval,
  clearInterval,
  fetch: fetchStub,
  addEventListener() {},
  removeEventListener() {},
  __TAURI__: { invoke: () => Promise.reject(new Error('noop')) },
};

windowObj.window = windowObj;
windowObj.globalThis = windowObj;

vm.createContext(windowObj);
const code = fs.readFileSync(path.join(__dirname, 'common.js'), 'utf8');
vm.runInContext(code, windowObj, { filename: 'common.js' });

const read = windowObj.ARW && windowObj.ARW.read;
if (!read) {
  throw new Error('ARW.read not initialised');
}

function resetStore() {
  read._store.clear();
  read._subs.clear();
  read._next = 1;
}

async function run() {
  // Basic add/replace/remove flows
  resetStore();
  const model = {};
  read._store.set('projects', model);
  read._applyOp(model, { op: 'add', path: '/items', value: [] });
  read._applyOp(model, { op: 'add', path: '/items/-', value: { name: 'alpha' } });
  read._applyOp(model, { op: 'add', path: '/items/0/notes', value: { content: 'hi' } });
  assert.strictEqual(model.items.length, 1);
  assert.strictEqual(model.items[0].name, 'alpha');
  assert.strictEqual(model.items[0].notes.content, 'hi');

  read._applyOp(model, { op: 'replace', path: '/items/0/notes/content', value: 'hello' });
  assert.strictEqual(model.items[0].notes.content, 'hello');

  read._applyOp(model, { op: 'remove', path: '/items/0/notes' });
  assert.strictEqual(model.items[0].notes, undefined);

  // copy/move semantics
  read._applyOp(model, { op: 'add', path: '/items/0/routes', value: ['a', 'b'] });
  read._applyOp(model, { op: 'copy', path: '/items/1', from: '/items/0' });
  assert.strictEqual(model.items[1].routes.length, 2);
  read._applyOp(model, { op: 'move', path: '/items/0/routes/0', from: '/items/0/routes/1' });
  assert.strictEqual(model.items[0].routes[0], 'b');

  // pointer auto-creation for nested objects
  read._applyOp(model, { op: 'add', path: '/meta/info/value', value: 42 });
  assert.strictEqual(model.meta.info.value, 42);

  // verify emit/subscription
  let observed = null;
  const subId = read.subscribe('projects', (val) => { observed = val; });
  read._store.set('projects', model);
  read._emit('projects');
  assert.ok(observed === model);
  read.unsubscribe(subId);

  console.log('ARW.read store patch tests passed');

  const { ARW } = windowObj;
  assert.ok(ARW, 'ARW helpers missing');

  // normalizeBase should lowercase host, drop trailing slash, and add http:// when missing
  assert.strictEqual(ARW.normalizeBase('HTTP://Example.COM:8091/'), 'http://example.com:8091');
  assert.strictEqual(ARW.normalizeBase('example.org:9000'), 'http://example.org:9000');
  assert.strictEqual(ARW.normalizeBase('https://Example.com'), 'https://example.com');

  // baseMeta returns local origin when no override is present
  const localMeta = ARW.baseMeta(9000);
  assert.strictEqual(localMeta.base, 'http://127.0.0.1:9000');
  assert.strictEqual(localMeta.override, false);
  assert.strictEqual(localMeta.port, 9000);

  // baseMeta reflects remote overrides and inferred ports
  windowObj.__ARW_BASE_OVERRIDE = 'https://REMOTE.example.com';
  const remoteMeta = ARW.baseMeta();
  assert.strictEqual(remoteMeta.override, true);
  assert.strictEqual(remoteMeta.origin, 'https://remote.example.com');
  assert.strictEqual(remoteMeta.port, 443);
  delete windowObj.__ARW_BASE_OVERRIDE;

  // Connection token resolution prefers exact match token, trimmed, then falls back to admin token
  ARW._prefsCache.clear();
  ARW._prefsCache.set('launcher', {
    connections: [
      { name: 'remote', base: 'http://Example.com:8091/', token: ' conn-token ' },
    ],
  });
  let token = await ARW.connections.tokenFor('http://example.com:8091');
  assert.strictEqual(token, 'conn-token');

  ARW.clearBaseOverride();
  const storedOverride = ARW.setBaseOverride('https://REMOTE.example.com:9001/');
  assert.strictEqual(storedOverride, 'https://remote.example.com:9001');
  assert.strictEqual(ARW.baseOverride(), 'https://remote.example.com:9001');
  ARW.clearBaseOverride();
  assert.strictEqual(ARW.baseOverride(), '');

  ARW._prefsCache.clear();
  ARW._prefsCache.set('launcher', {
    connections: [
      { name: 'remote', base: 'http://example.com:8091/', token: '   ' },
    ],
    adminToken: ' admin-secret ',
  });
  token = await ARW.connections.tokenFor('http://other.example:8091');
  assert.strictEqual(token, 'admin-secret');

  ARW._prefsCache.clear();
  ARW._prefsCache.set('launcher', {
    connections: [
      { name: 'remote', base: 'http://example.com:8091/', token: '   ' },
    ],
    adminToken: '   ',
  });
  token = await ARW.connections.tokenFor('http://example.com:8091/');
  assert.strictEqual(token, null);

  console.log('ARW.connections helpers tests passed');

  // SSE indicator badge behavior
  const badgeNode = makeNode('span');
  const indicatorHandle = windowObj.ARW.sse.indicator(badgeNode, { prefix: 'SSE', refreshMs: 600, staleMs: 500 });
  assert.ok(badgeNode.classList.contains('sse-badge'), 'indicator should add badge styling');
  assert.strictEqual(badgeNode.getAttribute('role'), 'status');
  assert.strictEqual(badgeNode.getAttribute('aria-live'), 'polite');

  const subsList = Array.from(windowObj.ARW.sse._subs.values());
  const statusSub = subsList[subsList.length - 1];
  assert.ok(statusSub && typeof statusSub.cb === 'function', 'indicator subscription missing');

  const originalLastEventAt = windowObj.ARW.sse.lastEventAt;
  let lastEventAgoMs = 0;
  windowObj.ARW.sse.lastEventAt = () => Date.now() - lastEventAgoMs;

  statusSub.cb({ kind: '*status*', env: { status: 'connecting', retryIn: 1500, changedAt: Date.now() - 500 } });
  assert.ok(badgeNode.textContent.includes('retry in 1.5'), 'connecting label should include retry window');
  assert.strictEqual(badgeNode.getAttribute('aria-label'), badgeNode.textContent, 'aria-label should mirror text');

  lastEventAgoMs = 0;
  const nowOpen = Date.now();
  statusSub.cb({ kind: '*status*', env: { status: 'open', changedAt: nowOpen } });
  assert.ok(/last event/.test(badgeNode.textContent), 'open label should include last event timing');

  lastEventAgoMs = 3000;
  await new Promise((resolve) => setTimeout(resolve, 700));
  assert.ok(/3s ago|last event 3/.test(badgeNode.textContent), 'refresh timer should update relative time');
  assert.strictEqual(badgeNode.dataset.state, 'stale', 'badge should mark stale state');
  indicatorHandle.dispose();
  windowObj.ARW.sse.lastEventAt = originalLastEventAt;

  // downloadPercent helper
  assert.strictEqual(ARW.util.downloadPercent({ percent: '42.5' }), 42.5);
  assert.strictEqual(ARW.util.downloadPercent({ percent: '85%' }), 85);
  assert.strictEqual(ARW.util.downloadPercent({ progress: -12 }), 0);
  assert.strictEqual(ARW.util.downloadPercent({ progress: 120 }), 100);
  assert.strictEqual(ARW.util.downloadPercent({ downloaded: 50, total: 200 }), 25);
  assert.strictEqual(ARW.util.downloadPercent({}), null);

  // miniDownloads renders using normalized percent / fallback math
  const originalGetElementById = documentNode.getElementById;
  const originalCreateElement = documentNode.createElement;
  const nodeMap = new Map();
  const registerNode = (id, node) => { node.id = id; nodeMap.set(id, node); return node; };
  documentNode.getElementById = (id) => {
    if (!nodeMap.has(id)) {
      nodeMap.set(id, makeNode());
      nodeMap.get(id).id = id;
    }
    return nodeMap.get(id);
  };
  documentNode.createElement = (tag) => makeNode(tag);

  const dlRoot = registerNode('dlmini', makeNode('div'));
  const dlProg = registerNode('dlprog', makeNode('pre'));
  registerNode('dlbars', makeNode('div'));
  registerNode('disk', makeNode('div'));

  const indexCode = fs.readFileSync(path.join(__dirname, 'index.js'), 'utf8');
  vm.runInContext(indexCode, windowObj, { filename: 'index.js' });
  assert.strictEqual(typeof windowObj.miniDownloads, 'function', 'miniDownloads missing');

  const initialSubs = windowObj.ARW.sse._subs.size;
  windowObj.miniDownloads();
  assert.strictEqual(windowObj.ARW.sse._subs.size, initialSubs + 1, 'miniDownloads did not subscribe to SSE');

  const subs = Array.from(windowObj.ARW.sse._subs.values());
  const handler = subs[subs.length - 1].cb;
  assert.strictEqual(typeof handler, 'function', 'SSE handler missing');

  const originalNow = Date.now;
  Date.now = () => 1_000;

  handler({ kind: 'models.download.progress', env: { payload: { id: 'model-1', percent: 42.4, downloaded: 0 } } });
  const badgeOne = dlRoot.children[0];
  const labelOne = badgeOne?.children?.[1];
  assert.ok(labelOne, 'mini download badge missing');
  assert.strictEqual(labelOne.textContent, ' model-1 42%');

  handler({ kind: 'models.download.progress', env: { payload: { id: 'model-2', downloaded: 25, total: 100 } } });
  const badgeTwo = dlRoot.children[1];
  const labelTwo = badgeTwo?.children?.[1];
  assert.ok(labelTwo, 'second mini download badge missing');
  assert.strictEqual(labelTwo.textContent, ' model-2 25%');

  Date.now = originalNow;
  documentNode.getElementById = originalGetElementById;
  documentNode.createElement = originalCreateElement;

  console.log('Launcher mini download normalization tests passed');
}

run().catch((err) => {
  console.error(err);
  process.exit(1);
});
