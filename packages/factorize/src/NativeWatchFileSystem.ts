import type { NativeWatcher, JsWatcherOptions } from "@factorize/binding";

let binding: typeof import("@factorize/binding") | undefined;

function loadBinding() {
  if (!binding) {
    binding = require("@factorize/binding");
  }
  return binding!;
}

export interface WatchOptions {
  aggregateTimeout?: number;
}

export interface WatchResult {
  changedFiles: string[];
  removedFiles: string[];
}

/**
 * rspack의 NativeWatchFileSystem 최소 버전
 * Rust의 FsWatcher를 NAPI를 통해 JS에서 사용하는 래퍼
 */
export class NativeWatchFileSystem {
  #inner: NativeWatcher;

  constructor(options: WatchOptions = {}) {
    const { NativeWatcher } = loadBinding();
    this.#inner = new NativeWatcher({
      aggregateTimeout: options.aggregateTimeout,
    });
  }

  /**
   * 경로들을 감시 시작
   *
   * @param paths - 감시할 경로 목록
   * @param callback - 집계된 이벤트 콜백 (aggregate timeout 후 호출)
   * @param callbackUndelayed - 개별 이벤트 콜백 (즉시 호출)
   */
  watch(
    paths: string[],
    callback: (err: Error | null, result: WatchResult) => void,
    callbackUndelayed: (path: string) => void
  ): void {
    this.#inner.watch(paths, callback, callbackUndelayed);
  }

  pause(): void {
    this.#inner.pause();
  }

  async close(): Promise<void> {
    await this.#inner.close();
  }
}
