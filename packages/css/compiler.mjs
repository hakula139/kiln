#!/usr/bin/env node
import { readFile, writeFile } from "node:fs/promises";
import { delimiter, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const modules = join(dirname(fileURLToPath(import.meta.url)), "node_modules");
process.env.NODE_PATH = [process.env.NODE_PATH, modules]
  .filter(Boolean)
  .join(delimiter);
const { compile } = await import("@tailwindcss/node");
const { Scanner } = await import("@tailwindcss/oxide");

const [input, output] = process.argv.slice(2);
const base = dirname(input);
const compiler = await compile(await readFile(input, "utf8"), {
  base,
  from: input,
  onDependency() {},
  shouldRewriteUrls: true,
});
const sources = [...compiler.sources];
if (compiler.root !== "none") {
  sources.push({
    ...(compiler.root ?? { base, pattern: "**/*" }),
    negated: false,
  });
}
const scanner = new Scanner({ sources });
await writeFile(output, compiler.build(scanner.scan()));
