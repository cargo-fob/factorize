import { Compiler } from "./Compiler";
import { NativeWatchFileSystem } from "./NativeWatchFileSystem";
import { sum } from "@factorize/binding";
import * as path from "path";

console.log("sum(3, 4) =", sum(3, 4));

const compiler = new Compiler({
  name: "factorize",
  entry: "./src/index.ts",
});

console.log("compiler.name:", compiler.name);
console.log("compiler.entry:", compiler.entry);
console.log("compiler.compile():", compiler.compile());

const watchDir = path.resolve(__dirname, "..");

console.log(`\nWatching: ${watchDir}`);
console.log("Try editing a file in that directory...\n");

const watcher = new NativeWatchFileSystem({
  aggregateTimeout: 200,
});

watcher.watch(
  [watchDir],
  (err, result) => {
    if (err) {
      console.error("Watch error:", err);
      return;
    }
    console.log("[aggregate] changed:", result.changedFiles);
    console.log("[aggregate] removed:", result.removedFiles);
  },
  (changedPath) => {
    console.log("[immediate] changed:", changedPath);
  }
);

// Ctrl+C로 종료 시 watcher 정리
process.on("SIGINT", async () => {
  console.log("\nClosing watcher...");
  await watcher.close();
  process.exit(0);
});
