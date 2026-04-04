import type { JsCompiler } from "@factorize/binding";

let binding: typeof import("@factorize/binding") | undefined;

function loadBinding() {
  if (!binding) {
    binding = require("@factorize/binding");
  }
  return binding!;
}

export interface CompilerOptions {
  name: string;
  entry: string;
}

export class Compiler {
  #instance: JsCompiler;

  constructor(options: CompilerOptions) {
    const instanceBinding = loadBinding();
    this.#instance = new instanceBinding.JsCompiler(
      options.name,
      options.entry
    );
  }

  get name(): string {
    return this.#instance.name;
  }

  get entry(): string {
    return this.#instance.entry;
  }

  compile(): string {
    return this.#instance.compile();
  }
}
