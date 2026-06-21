import { hello } from "./dep.js";
import { VERSION } from "./meta/version.js";

console.log(hello(), VERSION);
