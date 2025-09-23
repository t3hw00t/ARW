#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const vm = require('vm');
const assert = require('assert');

function makeNode() {
  const node = {
    style: {},
    className: '',
    innerHTML: '',
    textContent: '',
    value: '',
    dataset: {},
    children: [],
    classList: {
      add() {},
      remove() {},
      toggle() {},
      contains() { return false; }
    },
    appendChild(child) { this.children.push(child); return child; },
    prepend(child) { this.children.unshift(child); return child; },
    removeChild(child) { this.children = this.children.filter((c) => c !== child); },
    insertAdjacentHTML() {},
    setAttribute() {},
    getAttribute() { return null; },
    addEventListener() {},
    removeEventListener() {},
    querySelector() { return makeNode(); },
    querySelectorAll() { return []; },
    cloneNode() { return makeNode(); },
    focus() {},
    blur() {},
    submit() {},
    click() {},
  };
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
