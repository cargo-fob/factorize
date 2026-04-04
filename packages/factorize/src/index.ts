import { Compiler } from "./Compiler";
import { sum } from "@factorize/binding";

console.log("sum(3, 4) =", sum(3, 4));

const compiler = new Compiler({
  name: "factorize",
  entry: "./src/index.ts",
});

console.log("compiler.name:", compiler.name);
console.log("compiler.entry:", compiler.entry);
console.log("compiler.compile():", compiler.compile());
