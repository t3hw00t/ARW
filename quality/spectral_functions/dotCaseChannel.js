'use strict';

const DEFAULT_PATTERN = /^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$/;

module.exports = (targetVal, opts, context) => {
  if (targetVal === null || typeof targetVal !== 'object' || Array.isArray(targetVal)) {
    return;
  }

  const pattern = (() => {
    if (opts && typeof opts.pattern === 'string') {
      try {
        return new RegExp(opts.pattern);
      } catch (_err) {
        // fall through to default if pattern is invalid
      }
    }
    return DEFAULT_PATTERN;
  })();

  const results = [];
  for (const key of Object.keys(targetVal)) {
    if (typeof key !== 'string') {
      continue;
    }

    if (!pattern.test(key)) {
      results.push({
        message: `Channel '${key}' should be dot.case (e.g., models.download.progress)`,
        path: [...context.path, key],
      });
    }
  }

  return results.length > 0 ? results : undefined;
};
