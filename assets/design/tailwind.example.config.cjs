// Example Tailwind config that imports ARW tokens
// Generate tokens first: `just tokens-tailwind` (writes assets/design/tailwind.tokens.json)

const fs = require('fs');
const path = require('path');
const tokensPath = path.join(__dirname, 'tailwind.tokens.json');
const hasTokens = fs.existsSync(tokensPath);
const tokens = hasTokens ? JSON.parse(fs.readFileSync(tokensPath, 'utf8')) : { theme: { colors: {} } };

module.exports = {
  content: ['./src/**/*.{js,ts,jsx,tsx,html}'],
  theme: {
    extend: {
      colors: {
        // Merge in tokens colors (brand/status/neutrals/surfaces)
        ...tokens.theme.colors
      }
    }
  },
  plugins: []
};

