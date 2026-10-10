const fs = require("fs");
const s = fs.readFileSync("ui/vendor-src/molstar-viewer-5.12.0.js", "utf8");
console.log(s.slice(3802600, 3803500).replace(/\n/g, " "));
