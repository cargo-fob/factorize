import * as path from "path";
import { watch } from "./factorize";

const input = path.resolve(__dirname, "../fixture/entry.js");
console.log("watching (Rust-owned loop):", input, "\n");

const watcher = watch({ input });

watcher.on("bundle_end", (e) =>
  console.log(`[event] bundle_end — ${e.modules.length} modules`)
);
watcher.on("change", (e) =>
  console.log(`[event] change — ${path.basename(e.path)}`)
);
watcher.on("build_error", (e) => console.error(`[event] error — ${e.error}`));

process.on("SIGINT", async () => {
  console.log("\nclosing watcher");
  await watcher.close();
  process.exit(0);
});
