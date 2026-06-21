import type { BindingBundler, BindingOutput } from "@factorize/binding";
import { dispatchEvent, WatcherEmitter } from "./watch-emitter";

let binding: typeof import("@factorize/binding") | undefined;
function loadBinding() {
  if (!binding) binding = require("@factorize/binding");
  return binding!;
}

export interface FactorizeOptions {
  input: string;
}

export interface Plugin {
  moduleParsed?: (id: string) => void;
  /** 모듈 소스 변형. string이면 교체, null이면 통과. async 가능 */
  transform?: (code: string, id: string) => string | null | Promise<string | null>;
}

export type BuildOutput = BindingOutput;

/** 파이프라인 로직 없는 thin facade — 옵션/plugin을 napi로 넘기고 결과만 받는다 */
export class FactorizeBuild {
  #inner: BindingBundler;

  constructor(options: FactorizeOptions, plugins: Plugin[] = []) {
    const { BindingBundler } = loadBinding();
    this.#inner = new BindingBundler({ input: options.input });
    for (const p of plugins) {
      if (p.moduleParsed) this.#inner.onModuleParsed(p.moduleParsed);
      if (p.transform) this.#inner.onTransform(p.transform);
    }
  }

  build(): Promise<BuildOutput> {
    return this.#inner.build();
  }
}

export function factorize(
  options: FactorizeOptions,
  plugins: Plugin[] = []
): FactorizeBuild {
  return new FactorizeBuild(options, plugins);
}

/**
 * watch 루프는 Rust(BindingWatcher)가 소유. 파일 변경 시 rebuild가 Rust 안에서 일어나고,
 * JS는 emitter를 반환받아 이벤트만 구독한다.
 *   const w = watch({ input }); w.on("bundle_end", e => …)
 */
export function watch(options: FactorizeOptions): WatcherEmitter {
  const { BindingWatcher } = loadBinding();
  const emitter = new WatcherEmitter();
  const inner = new BindingWatcher({ input: options.input }, (event) =>
    dispatchEvent(emitter, event)
  );
  emitter.bind(inner);
  inner.run();
  return emitter;
}
