import { EventEmitter } from "events";
import type { BindingWatchEvent, BindingWatcher } from "@factorize/binding";

export interface BundleEndEvent {
  modules: string[];
  code: string;
}
export interface ChangeEvent {
  path: string;
}
export interface BuildErrorEvent {
  error: string;
}

/** Rust가 push한 이벤트를 Node EventEmitter로 fan-out하는 thin 레이어 */
export class WatcherEmitter extends EventEmitter {
  #inner?: BindingWatcher;

  bind(inner: BindingWatcher): void {
    this.#inner = inner;
  }

  async close(): Promise<void> {
    await this.#inner?.close();
    this.removeAllListeners();
  }

  on(event: "bundle_end", handler: (e: BundleEndEvent) => void): this;
  on(event: "change", handler: (e: ChangeEvent) => void): this;
  on(event: "build_error", handler: (e: BuildErrorEvent) => void): this;
  on(event: string, handler: (...args: any[]) => void): this {
    return super.on(event, handler);
  }
}

/** Rust의 단일 listener — BindingWatchEvent를 emitter로 재방출.
 *  'error'는 EventEmitter 예약어라(리스너 없으면 throw) 'build_error'로 쓴다 */
export function dispatchEvent(
  emitter: WatcherEmitter,
  event: BindingWatchEvent
): void {
  switch (event.kind) {
    case "bundle_end":
      emitter.emit("bundle_end", {
        modules: event.modules ?? [],
        code: event.code ?? "",
      });
      break;
    case "change":
      emitter.emit("change", { path: event.path ?? "" });
      break;
    case "error":
      emitter.emit("build_error", { error: event.error ?? "" });
      break;
  }
}
