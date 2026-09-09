#!/usr/bin/env node

import { readdirSync, statSync } from "node:fs";
import { extname, join } from "node:path";

const budgets = [
  { root: "apps/desktop/dist", javascript: 300 * 1024, css: 20 * 1024, total: 350 * 1024 },
  { root: "apps/family-display/dist", javascript: 225 * 1024, css: 12 * 1024, total: 250 * 1024 },
];

for (const budget of budgets) {
  const sizes = { javascript: 0, css: 0, total: 0 };
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) visit(path);
      else if (entry.isFile()) {
        const size = statSync(path).size;
        sizes.total += size;
        if (extname(path) === ".js") sizes.javascript += size;
        if (extname(path) === ".css") sizes.css += size;
      }
    }
  };
  visit(budget.root);
  for (const metric of ["javascript", "css", "total"]) {
    if (sizes[metric] > budget[metric]) {
      throw new Error(`${budget.root} ${metric} exceeds its release budget`);
    }
  }
  process.stdout.write(`${budget.root}: ${sizes.total} bytes (${sizes.javascript} JS, ${sizes.css} CSS)\n`);
}
