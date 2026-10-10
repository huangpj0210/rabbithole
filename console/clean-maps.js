// 生产产物不携带 sourcemap(避免被 rust-embed 嵌入,二进制体积敏感)
const fs = require('node:fs');
fs.readdirSync('dist')
  .filter((f) => f.endsWith('.map'))
  .forEach((f) => fs.rmSync(`dist/${f}`));
