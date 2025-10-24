#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const assert = require('assert');

function makeNode(tag = 'div') {
  const node = {
    tagName: String(tag || 'div').toUpperCase(),
    id: '',
    className: '',
    dataset: {},
    attributes: Object.create(null),
    children: [],
    parentNode: null,
    style: {},
    _innerHTML: '',
    _textContent: '',
    appendChild(child) {
      if (!child) return child;
      child.parentNode = this;
      this.children.push(child);
      return child;
    },
    append(...children) {
      children.forEach((child) => this.appendChild(child));
    },
    removeChild(child) {
      this.children = this.children.filter((c) => c !== child);
      if (child) child.parentNode = null;
    },
    querySelector() { return null; },
    querySelectorAll() { return []; },
    setAttribute(name, value) {
      const val = String(value ?? '');
      this.attributes[name] = val;
      if (name === 'id') this.id = val;
      if (name.startsWith('data-')) {
        const key = name.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase());
        this.dataset[key] = val;
      }
    },
    getAttribute(name) {
      return Object.prototype.hasOwnProperty.call(this.attributes, name)
        ? this.attributes[name]
        : null;
    },
  };
  Object.defineProperty(node, 'classList', {
    value: {
      add() {},
      remove() {},
      toggle() {},
      contains() { return false; },
    },
  });
  Object.defineProperty(node, 'innerHTML', {
    get() { return node._innerHTML; },
    set(value) {
      node._innerHTML = String(value ?? '');
      node.children = [];
    },
  });
  Object.defineProperty(node, 'textContent', {
    get() { return node._textContent; },
    set(value) { node._textContent = String(value ?? ''); },
  });
  return node;
}

function readSource() {
  const file = path.join(__dirname, 'common.js');
  return fs.readFileSync(file, 'utf8');
}

function extractArrowFunction(source, name) {
  const marker = `const ${name} = `;
  const start = source.indexOf(marker);
  if (start === -1) throw new Error(`Unable to locate ${name}`);
  const afterMarker = start + marker.length;
  const nextBlank = source.indexOf('\n\n', afterMarker);
  const segment = source.slice(afterMarker, nextBlank === -1 ? source.length : nextBlank).trim();
  const expression = segment.endsWith(';') ? segment.slice(0, -1) : segment;
  return eval(expression); // eslint-disable-line no-eval
}

function findMatchingBrace(source, startIndex) {
  let depth = 1;
  let inSingle = false;
  let inDouble = false;
  let inTemplate = false;
  let templateDepth = 0;
  let escape = false;
  for (let i = startIndex + 1; i < source.length; i += 1) {
    const ch = source[i];
    const next = source[i + 1];

    if (inSingle) {
      if (!escape && ch === '\'') inSingle = false;
      escape = !escape && ch === '\\';
      continue;
    }
    if (inDouble) {
      if (!escape && ch === '"') inDouble = false;
      escape = !escape && ch === '\\';
      continue;
    }
    if (inTemplate) {
      if (escape) {
        escape = false;
        continue;
      }
      if (ch === '\\') {
        escape = true;
        continue;
      }
      if (templateDepth === 0 && ch === '`') {
        inTemplate = false;
        continue;
      }
      if (templateDepth === 0 && ch === '$' && next === '{') {
        templateDepth = 1;
        i += 1;
        continue;
      }
      if (templateDepth > 0) {
        if (ch === '{') {
          templateDepth += 1;
          continue;
        }
        if (ch === '}') {
          templateDepth -= 1;
          continue;
        }
      }
      if (ch === '\'') { inSingle = true; continue; }
      if (ch === '"') { inDouble = true; continue; }
      continue;
    }

    if (escape) {
      escape = false;
      continue;
    }
    if (ch === '\\') {
      escape = true;
      continue;
    }
    if (ch === '/' && next === '/') {
      i += 2;
      while (i < source.length && source[i] !== '\n') i += 1;
      continue;
    }
    if (ch === '/' && next === '*') {
      i += 2;
      while (i < source.length && !(source[i] === '*' && source[i + 1] === '/')) i += 1;
      i += 1;
      continue;
    }
    if (ch === '\'') { inSingle = true; continue; }
    if (ch === '"') { inDouble = true; continue; }
    if (ch === '`') { inTemplate = true; templateDepth = 0; continue; }
    if (ch === '{') {
      depth += 1;
      continue;
    }
    if (ch === '}') {
      depth -= 1;
      if (depth === 0) return i;
      continue;
    }
  }
  throw new Error('Unbalanced braces while extracting method');
}

function extractMethodFunction(source, name) {
  const marker = `${name}(`;
  let methodStart = source.indexOf(marker);
  while (methodStart !== -1 && source[methodStart - 1] === '.') {
    methodStart = source.indexOf(marker, methodStart + marker.length);
  }
  if (methodStart === -1) throw new Error(`Unable to locate method ${name}`);
  const remainder = source.slice(methodStart);
  const headerRegex = new RegExp(`^${name}\\s*\\(([^)]*)\\)\\s*\\{`);
  const headerMatch = headerRegex.exec(remainder);
  if (!headerMatch) throw new Error(`Unable to parse signature for ${name}`);
  const params = headerMatch[1].trim();
  const braceOffset = headerMatch[0].lastIndexOf('{');
  const braceIndex = methodStart + braceOffset;
  const bodyEnd = findMatchingBrace(source, braceIndex);
  const body = source.slice(braceIndex + 1, bodyEnd).trim();
  return new Function(params, body); // eslint-disable-line no-new-func
}

function run() {
  const rawSource = readSource();
  const source = rawSource.replace(/\r/g, '\n');
  const escapeHtml = extractArrowFunction(source, 'escapeHtml');
  const parseDate = extractArrowFunction(source, 'parseDate');
  const formatRelative = extractArrowFunction(source, 'formatRelative');
  const updateHistoryMeta = extractMethodFunction(source, 'updateHistoryMeta');
  const setRetentionInfo = extractMethodFunction(source, 'setRetentionInfo');
  const renderMetrics = extractMethodFunction(source, 'renderMetrics');

  global.escapeHtml = escapeHtml;
  global.formatRelative = formatRelative;
  global.parseDate = parseDate;

  class PersonaPanelHarness {
    constructor() {
      this.metrics = makeNode('div');
      this.history = makeNode('ul');
      this.historyMeta = makeNode('div');
      this.retentionNoteDefault = 'Retention defaults to 50 samples (ARW_PERSONA_VIBE_HISTORY_RETAIN).';
      this.retentionNote = this.retentionNoteDefault;
      this.retainMax = null;
      this._historyMetaPrefix = '';
    }
  }

  PersonaPanelHarness.prototype.updateHistoryMeta = updateHistoryMeta;
  PersonaPanelHarness.prototype.setRetentionInfo = setRetentionInfo;
  PersonaPanelHarness.prototype.renderMetrics = function(metrics, detail, message) {
    return renderMetrics.call(this, metrics, detail, message);
  };

  const panel = new PersonaPanelHarness();
  const detail = {
    vibe_metrics_preview: {
      total_feedback: 3,
      signal_counts: { warmer: 2, cooler: 1 },
      signal_strength: { warmer: 0.8, cooler: 0.4 },
      signal_weights: { warmer: 4, cooler: 1.5 },
      average_strength: 0.6,
      last_signal: 'warmer',
      last_strength: 0.9,
      last_updated: null,
      retain_max: 40,
    },
    context_bias_preview: {
      lane_priorities: { retrieval: 0.3, persona: -0.2 },
      slot_overrides: { story: 3, audit: 1 },
      min_score_delta: 0.15,
    },
  };

  panel.renderMetrics(null, detail, null);

  assert.ok(panel.metrics.innerHTML.includes('Average strength'), 'expected average strength metric');
  assert.ok(panel.metrics.innerHTML.includes('Signals'), 'signals card missing');
  assert.ok(panel.metrics.innerHTML.includes('w 4.00'), 'weighted strength missing');
  assert.ok(panel.metrics.innerHTML.includes('Lane priorities'), 'lane priorities not rendered');
  assert.ok(panel.metrics.innerHTML.includes('story'), 'slot overrides missing');
  assert.strictEqual(panel.retainMax, 40, 'retainMax should adopt preview retain_max');
  assert.ok(
    panel.historyMeta.textContent.includes('Retention capped at 40 samples'),
    'retention note missing expected copy'
  );

  panel.renderMetrics(null, { vibe_metrics_preview: null, context_bias_preview: null }, 'No data');
  assert.ok(panel.metrics.innerHTML.includes('No data'), 'fallback message should be rendered');

  console.log('Persona preview telemetry UI tests passed');

  delete global.escapeHtml;
  delete global.formatRelative;
  delete global.parseDate;
}

run();
