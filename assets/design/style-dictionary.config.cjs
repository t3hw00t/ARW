/** Optional Style Dictionary config (Node/NPM required)
 * Builds additional token artifacts from W3C tokens.
 * Run: npx style-dictionary build --config assets/design/style-dictionary.config.cjs
 */

const StyleDictionary = require('style-dictionary');

// Custom filter: only color tokens
StyleDictionary.registerFilter({
  name: 'isColor',
  matcher: (token) => token?.type === 'color' || token?.attributes?.category === 'color'
});

// Custom format: dark overrides CSS using shared dark neutrals
StyleDictionary.registerFormat({
  name: 'css/dark-overrides',
  formatter: () => {
    const dark = {
      '--surface': '#0f1115',
      '--surface-muted': '#0b0d11',
      '--color-ink': '#e5e7eb',
      '--color-line': '#1f232a',
    };
    const body = Object.entries(dark).map(([k,v])=>`    ${k}: ${v};`).join('\n');
    return `@media (prefers-color-scheme: dark){\n  :root{\n${body}\n  }\n}`;
  }
});

function hexToRgbTuple(hex){
  const h = hex.replace('#','').trim();
  const n = h.length === 3 ? h.split('').map(c=>c+c).join('') : h;
  const r = parseInt(n.slice(0,2), 16);
  const g = parseInt(n.slice(2,4), 16);
  const b = parseInt(n.slice(4,6), 16);
  return [r,g,b];
}

// Derived variables: brand-copper-rgb and legacy aliases
StyleDictionary.registerFormat({
  name: 'css/derived',
  formatter: ({ dictionary }) => {
    const t = dictionary.allTokens;
    const brand = t.find(x => x.name === 'color-brand-copper');
    const [r,g,b] = brand ? hexToRgbTuple(brand.value) : [184,115,51];
    return `:root{\n  --brand-copper-rgb: ${r},${g},${b};\n  --ink: var(--color-ink);\n  --muted: var(--color-muted);\n  --line: var(--color-line);\n}`;
  }
});

// Theme classes: .theme-light (all vars), .theme-dark (dark overrides)
StyleDictionary.registerFormat({
  name: 'css/theme-classes',
  formatter: ({ dictionary }) => {
    const vars = dictionary.allTokens.map(tok => `  --${tok.name}: ${tok.value};`).join('\n');
    const dark = [
      '  --surface: #0f1115;',
      '  --surface-muted: #0b0d11;',
      '  --color-ink: #e5e7eb;',
      '  --color-line: #1f232a;',
    ].join('\n');
    return `.theme-light{\n${vars}\n}\n\n.theme-dark{\n${dark}\n}`;
  }
});

module.exports = {
  source: [ 'assets/design/tokens.w3c.json' ],
  platforms: {
    css: {
      transformGroup: 'css',
      buildPath: 'assets/design/generated/',
      files: [
        { destination: 'tokens.css', format: 'css/variables', options: { selector: ':root', outputReferences: true } },
        { destination: 'tokens.dark.css', format: 'css/dark-overrides' },
        { destination: 'tokens.derived.css', format: 'css/derived' },
        { destination: 'tokens.theme.css', format: 'css/theme-classes' }
      ]
    },
    scss: {
      transformGroup: 'scss',
      buildPath: 'assets/design/generated/',
      files: [ { destination: 'tokens.scss', format: 'scss/variables' } ]
    },
    less: {
      transformGroup: 'less',
      buildPath: 'assets/design/generated/',
      files: [ { destination: 'tokens.less', format: 'less/variables' } ]
    },
    js: {
      transformGroup: 'js',
      buildPath: 'assets/design/generated/',
      files: [ { destination: 'tokens.mjs', format: 'javascript/module' } ]
    },
    json: {
      transformGroup: 'js',
      buildPath: 'assets/design/generated/',
      files: [
        { destination: 'tokens.json', format: 'json' },
        { destination: 'colors.json', format: 'json', filter: 'isColor' }
      ]
    },
    android: {
      transformGroup: 'android',
      buildPath: 'assets/design/generated/android/',
      files: [ { destination: 'colors.xml', format: 'android/colors' } ]
    },
    ios: {
      transformGroup: 'ios',
      buildPath: 'assets/design/generated/ios/',
      files: [ { destination: 'StyleDictionaryColor.swift', format: 'ios-swift/class.swift', className: 'StyleDictionaryColor' } ]
    }
  }
}
